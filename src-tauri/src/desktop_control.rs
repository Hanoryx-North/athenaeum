//! Same-user, loopback-only command transport for the running desktop.
//! This is deliberately separate from the knowledge bridge and its permissions.
use std::collections::HashMap;
use std::io::Read;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

const MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>;

#[derive(Default)]
pub struct DesktopControl {
    pending: Pending,
    frontend: Arc<Mutex<Option<String>>>,
}

#[tauri::command]
pub fn desktop_control_ready(
    session: String,
    ready: bool,
    state: tauri::State<'_, DesktopControl>,
) -> Result<(), String> {
    let mut frontend = state.frontend.lock().map_err(|e| e.to_string())?;
    if ready {
        *frontend = Some(session);
    } else if frontend.as_ref() == Some(&session) {
        *frontend = None;
    }
    Ok(())
}

fn status(app: &tauri::AppHandle, ready: bool) -> Value {
    let windows = app.webview_windows().into_iter().map(|(label, window)| json!({
        "label": label, "visible": window.is_visible().ok(), "url": window.url().ok().map(|url| url.to_string()),
    })).collect::<Vec<_>>();
    json!({"pid": std::process::id(), "schemaVersion": 1, "frontendReady": ready, "windows": windows})
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ControlRequest {
    id: String,
    operation: String,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    operation: String,
    #[serde(default)]
    payload: Value,
}

#[tauri::command]
pub fn desktop_control_respond(
    id: String,
    response: Value,
    state: tauri::State<'_, DesktopControl>,
) -> Result<(), String> {
    if let Some(sender) = state.pending.lock().map_err(|e| e.to_string())?.remove(&id) {
        let _ = sender.send(response);
    }
    Ok(())
}

#[tauri::command]
pub fn desktop_control_quit(app: tauri::AppHandle) {
    // Leave time for the authenticated caller to receive the acknowledgement.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        app.exit(0);
    });
}

fn respond(request: tiny_http::Request, status: u16, value: Value) {
    let response = tiny_http::Response::from_string(value.to_string())
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap());
    let _ = request.respond(response);
}

pub fn start(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let address = server.server_addr().to_string();
    let token = uuid::Uuid::new_v4().to_string();
    let directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&directory)?;
    let descriptor = directory.join("desktop-control.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&descriptor)?;
    file.write_all(
        json!({
            "schemaVersion": 1, "url": format!("http://{address}"),
            "token": token, "pid": std::process::id(),
        })
        .to_string()
        .as_bytes(),
    )?;
    let pending = app.state::<DesktopControl>().pending.clone();
    let frontend = app.state::<DesktopControl>().frontend.clone();
    let app = app.clone();
    let permits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            // Never accept browser cross-origin requests, even with a leaked token.
            let browser_request = request
                .headers()
                .iter()
                .any(|h| h.field.equiv("Origin") || h.field.equiv("Sec-Fetch-Site"));
            let authorized = request.headers().iter().any(|h| {
                h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {token}")
            });
            if browser_request || !authorized {
                respond(
                    request,
                    403,
                    json!({"error": "Desktop authorization required"}),
                );
                continue;
            }
            if request.method() == &tiny_http::Method::Get && request.url() == "/health" {
                respond(
                    request,
                    200,
                    status(
                        &app,
                        frontend
                            .lock()
                            .map(|value| value.is_some())
                            .unwrap_or(false),
                    ),
                );
                continue;
            }
            if request.method() != &tiny_http::Method::Post || request.url() != "/command" {
                respond(request, 404, json!({"error": "Unknown desktop endpoint"}));
                continue;
            }
            if permits.load(std::sync::atomic::Ordering::Relaxed) >= 16 {
                respond(
                    request,
                    429,
                    json!({"error": "Too many pending desktop commands"}),
                );
                continue;
            }
            let mut body = Vec::new();
            if request
                .as_reader()
                .take(MAX_REQUEST_BYTES + 1)
                .read_to_end(&mut body)
                .is_err()
                || body.len() as u64 > MAX_REQUEST_BYTES
            {
                respond(
                    request,
                    413,
                    json!({"error": "Invalid or oversized desktop request"}),
                );
                continue;
            }
            let input: Input = match serde_json::from_slice(&body) {
                Ok(input) => input,
                Err(error) => {
                    respond(request, 400, json!({"error": error.to_string()}));
                    continue;
                }
            };
            let ready = frontend
                .lock()
                .map(|value| value.is_some())
                .unwrap_or(false);
            if input.operation == "show" {
                let result = app
                    .get_webview_window("main")
                    .ok_or("Main window is not initialized")
                    .and_then(|window| {
                        window
                            .show()
                            .map_err(|_| "Could not show the main window")?;
                        window
                            .unminimize()
                            .map_err(|_| "Could not restore the main window")
                    });
                respond(
                    request,
                    200,
                    match result {
                        Ok(()) => json!({"result": {"shown": true}}),
                        Err(error) => json!({"error":error}),
                    },
                );
                continue;
            }
            if !ready {
                if input.operation == "status" {
                    respond(request, 200, json!({"result": status(&app, false)}));
                } else {
                    respond(
                        request,
                        503,
                        json!({"error": "The desktop frontend is not ready; inspect wiki desktop status"}),
                    );
                }
                continue;
            }
            let id = uuid::Uuid::new_v4().to_string();
            let (sender, receiver) = mpsc::channel();
            let Ok(mut entries) = pending.lock() else {
                respond(request, 500, json!({"error": "Desktop queue unavailable"}));
                continue;
            };
            entries.insert(id.clone(), sender);
            drop(entries);
            let result = app.emit_to(
                "main",
                "desktop-control-request",
                ControlRequest {
                    id: id.clone(),
                    operation: input.operation,
                    payload: input.payload,
                },
            );
            if let Err(error) = result {
                if let Ok(mut entries) = pending.lock() {
                    entries.remove(&id);
                }
                respond(request, 503, json!({"error": error.to_string()}));
                continue;
            }
            let pending = pending.clone();
            let permits = permits.clone();
            permits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::thread::spawn(move || {
                match receiver.recv_timeout(REQUEST_TIMEOUT) {
                    Ok(value) => respond(request, 200, value),
                    Err(_) => {
                        if let Ok(mut entries) = pending.lock() {
                            entries.remove(&id);
                        }
                        respond(
                            request,
                            504,
                            json!({"error": "Desktop response timed out. The operation may still be running; inspect state before retrying."}),
                        );
                    }
                }
                permits.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
    });
    Ok(())
}
