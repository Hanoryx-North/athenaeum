//! The desktop's hands on the `athenaeum` command-line tool.
//!
//! Connecting coding agents, signing in to the Library and syncing all belong to the CLI: it holds
//! the Library client and the credentials, and this window makes no network request to a hosted
//! service. What the window can do — on an explicit click, never in the background — is run that CLI
//! as a subprocess and show what it says. This module is that and nothing more.
//!
//! The set of things the window may ask for is closed ([`CliJob`]), the same way
//! [`crate::engine::EngineRequest`] closes the set of knowledge requests: a compromised webview gets
//! no general "run this command" door. Progress lines are forwarded as `cli-progress` events, and a
//! running job can be cancelled, because the browser sign-in step may wait for minutes.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::panic_guard::run_guarded_async;

const PROGRESS_EVENT: &str = "cli-progress";
const MAX_STDOUT_BYTES: u64 = 8 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 40;
const AGENTS: [&str; 3] = ["claude", "codex", "cursor"];

/// The job running now, if any. One at a time: two sign-ins or two syncs racing helps nobody.
#[derive(Default, Clone)]
pub struct CliRun {
    current: Arc<Mutex<Option<Arc<Mutex<Child>>>>>,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "athenaeum.exe"
    } else {
        "athenaeum"
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Where `install.mjs` puts Athenaeum by default; the same convention, so the app finds what the installer made.
fn default_prefix() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Athenaeum"))
    } else if cfg!(target_os = "macos") {
        home().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Athenaeum")
        })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".local").join("share")))
            .map(|dir| dir.join("athenaeum"))
    }
}

/// The installed `athenaeum` command: `ATHENAEUM_BIN`, the installer's directory, then `PATH`.
pub fn locate_cli() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("ATHENAEUM_BIN") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(prefix) = default_prefix() {
        candidates.push(prefix.join("bin").join(exe_name()));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join(exe_name())));
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

// -- what the window may ask for ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "job", rename_all = "snake_case", deny_unknown_fields)]
pub enum CliJob {
    /// Check every link: engine, project, Library, coding agents.
    Doctor,
    /// Create the knowledge base if needed and connect agents, and optionally the Library.
    Setup {
        /// Connect the Library. An empty string means the default server.
        #[serde(default)]
        library: Option<String>,
        #[serde(default)]
        auto_sync: Option<bool>,
        /// Which agents; empty means the ones installed.
        #[serde(default)]
        agents: Vec<String>,
        #[serde(default)]
        no_agents: bool,
        #[serde(default)]
        capture: Option<String>,
    },
    /// Where the project stands against the Library. Works offline.
    SyncStatus,
    /// Pull, then push what is left. Runs only when the person clicks.
    Sync,
    DisconnectAgents {
        agents: Vec<String>,
    },
    LibraryLogin {
        server: String,
    },
    LibraryLogout {
        server: String,
    },
    SyncLink {
        server: String,
        name: String,
        public: bool,
    },
    SyncClone {
        server: String,
        remote: String,
        destination: String,
    },
    SyncPush,
    SyncPull,
    SyncUnlink,
    SyncConflict {
        path: String,
    },
    SyncResolve {
        path: String,
        keep: ConflictChoice,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    Local,
    Remote,
    Both,
}

impl CliJob {
    fn label(&self) -> &'static str {
        match self {
            CliJob::Doctor => "doctor",
            CliJob::Setup { .. } => "setup",
            CliJob::SyncStatus => "sync_status",
            CliJob::Sync => "sync",
            CliJob::DisconnectAgents { .. } => "disconnect_agents",
            CliJob::LibraryLogin { .. } => "library_login",
            CliJob::LibraryLogout { .. } => "library_logout",
            CliJob::SyncLink { .. } => "sync_link",
            CliJob::SyncClone { .. } => "sync_clone",
            CliJob::SyncPush => "sync_push",
            CliJob::SyncPull => "sync_pull",
            CliJob::SyncUnlink => "sync_unlink",
            CliJob::SyncConflict { .. } => "sync_conflict",
            CliJob::SyncResolve { .. } => "sync_resolve",
        }
    }

    /// The exact argument vector. Nothing from the webview is interpolated into a shell.
    fn args(&self, workspace: &str) -> Result<Vec<String>, String> {
        let mut args: Vec<String> = vec!["-C".into(), workspace.into(), "--json".into()];
        match self {
            CliJob::Doctor => args.push("doctor".into()),
            CliJob::SyncStatus => args.extend(["sync".into(), "status".into()]),
            CliJob::Sync => args.push("sync".into()),
            CliJob::DisconnectAgents { agents } => {
                if agents.iter().any(|agent| !AGENTS.contains(&agent.as_str())) {
                    return Err("invalid_request: choose claude, codex or cursor".into());
                }
                args.push("disconnect-agents".into());
                if !agents.is_empty() {
                    args.extend(["--agents".into(), agents.join(",")]);
                }
            }
            CliJob::LibraryLogin { server } | CliJob::LibraryLogout { server } => {
                check_server(server)?;
                let verb = if matches!(self, CliJob::LibraryLogin { .. }) {
                    "login"
                } else {
                    "logout"
                };
                args.extend([
                    "library".into(),
                    verb.into(),
                    "--server".into(),
                    server.clone(),
                ]);
            }
            CliJob::SyncLink {
                server,
                name,
                public,
            } => {
                check_server(server)?;
                if name.is_empty() || name.len() > 100 || name.starts_with('-') {
                    return Err("invalid_request: enter a wiki name".into());
                }
                args.extend([
                    "sync".into(),
                    "link".into(),
                    "--server".into(),
                    server.clone(),
                    "--create".into(),
                    name.clone(),
                ]);
                if *public {
                    args.push("--public".into());
                }
            }
            CliJob::SyncClone {
                server,
                remote,
                destination,
            } => {
                check_server(server)?;
                if remote.is_empty()
                    || remote.len() > 255
                    || remote.starts_with('-')
                    || !Path::new(destination).is_absolute()
                {
                    return Err(
                        "invalid_request: choose a hosted wiki and an absolute destination folder"
                            .into(),
                    );
                }
                args.extend([
                    "sync".into(),
                    "clone".into(),
                    "--server".into(),
                    server.clone(),
                    "--".into(),
                    remote.clone(),
                    destination.clone(),
                ]);
            }
            CliJob::SyncPush => args.extend(["sync".into(), "push".into()]),
            CliJob::SyncPull => args.extend(["sync".into(), "pull".into()]),
            CliJob::SyncUnlink => args.extend(["sync".into(), "unlink".into()]),
            CliJob::SyncConflict { path } => {
                args.extend(["sync".into(), "conflict".into(), "--".into(), path.clone()])
            }
            CliJob::SyncResolve { path, keep } => {
                let choice = match keep {
                    ConflictChoice::Local => "local",
                    ConflictChoice::Remote => "remote",
                    ConflictChoice::Both => "both",
                };
                args.extend([
                    "sync".into(),
                    "resolve".into(),
                    "--keep".into(),
                    choice.into(),
                    "--".into(),
                    path.clone(),
                ]);
            }
            CliJob::Setup {
                library,
                auto_sync,
                agents,
                no_agents,
                capture,
            } => {
                args.extend(["setup".into(), "--yes".into()]);
                if let Some(mode) = capture {
                    if !["off", "instruct", "nudge"].contains(&mode.as_str()) {
                        return Err("invalid_request: unknown capture mode".into());
                    }
                    args.extend(["--capture".into(), mode.clone()]);
                }
                if let Some(server) = library {
                    args.push("--library".into());
                    if !server.is_empty() {
                        check_server(server)?;
                        args.push(server.clone());
                    }
                }
                match auto_sync {
                    Some(true) => args.push("--auto-sync".into()),
                    Some(false) => args.push("--no-auto-sync".into()),
                    None => {}
                }
                if *no_agents {
                    args.push("--no-agents".into());
                } else if !agents.is_empty() {
                    if let Some(bad) = agents
                        .iter()
                        .find(|agent| !AGENTS.contains(&agent.as_str()))
                    {
                        return Err(format!("invalid_request: unknown agent {bad:?}"));
                    }
                    args.push("--agents".into());
                    args.push(agents.join(","));
                }
            }
        }
        Ok(args)
    }
}

/// Tokens travel over https, or plain http to this machine; the CLI enforces it too, this fails earlier.
fn check_server(server: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(server)
        .map_err(|_| "invalid_request: enter a valid Library URL".to_string())?;
    let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (parsed.scheme() == "https" || (parsed.scheme() == "http" && local))
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        Ok(())
    } else {
        Err("invalid_request: the Library address must start with https:// (http:// is allowed only for this machine)".into())
    }
}

// -- answers --------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub agent_installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub exit_code: Option<i32>,
    /// What the CLI printed to stdout as JSON, when it printed JSON.
    pub output: Option<Value>,
    pub stderr: Vec<String>,
}

#[derive(Serialize, Clone)]
struct Progress<'a> {
    job: &'a str,
    line: &'a str,
    /// The line parsed, when the CLI wrote a structured event (for example the sign-in code).
    event: Option<Value>,
}

fn emit_line(app: &AppHandle, job: &str, line: &str) {
    let event = serde_json::from_str::<Value>(line)
        .ok()
        .filter(|value| value.get("event").is_some());
    let _ = app.emit(PROGRESS_EVENT, Progress { job, line, event });
}

/// Run `command` to the end, forwarding its progress, and register it so it can be cancelled.
fn run_process(
    app: &AppHandle,
    state: &CliRun,
    mut command: Command,
    job: &str,
    stream_stdout: bool,
) -> Result<JobResult, String> {
    let mut slot = state
        .current
        .lock()
        .map_err(|_| "cli state poisoned".to_string())?;
    if slot.is_some() {
        return Err("busy: another Athenaeum command is still running".into());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Installer children (npm, pnpm, Harness setup) must stop together.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing over the app
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {:?}: {error}", command.get_program()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    *slot = Some(child.clone());
    drop(slot);

    let out_app = app.clone();
    let out_job = job.to_string();
    let out_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stdout) = stdout {
            if stream_stdout {
                for line in BufReader::new(stdout.take(MAX_STDOUT_BYTES))
                    .lines()
                    .map_while(Result::ok)
                {
                    emit_line(&out_app, &out_job, &line);
                    text.push_str(&line);
                    text.push('\n');
                }
            } else {
                let _ = stdout.take(MAX_STDOUT_BYTES).read_to_string(&mut text);
            }
        }
        text
    });
    let err_app = app.clone();
    let err_job = job.to_string();
    let err_reader = std::thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                emit_line(&err_app, &err_job, &line);
                tail.push(line);
                if tail.len() > STDERR_TAIL_LINES {
                    tail.remove(0);
                }
            }
        }
        tail
    });

    let status = loop {
        match child
            .lock()
            .map_err(|_| "cli child poisoned".to_string())?
            .try_wait()
        {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(error.to_string()),
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if let Ok(mut slot) = state.current.lock() {
        *slot = None;
    }
    let text = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let status = status?;
    Ok(JobResult {
        exit_code: status.code(),
        output: serde_json::from_str(text.trim()).ok(),
        stderr,
    })
}

// -- commands -------------------------------------------------------------------------------------

#[tauri::command]
pub async fn cli_status() -> Result<CliStatus, String> {
    run_guarded_async("cli_status", async {
        tauri::async_runtime::spawn_blocking(|| {
            let agent_installed = default_prefix()
                .map(|p| p.join("agent").join("install.json").is_file())
                .unwrap_or(false);
            let Some(path) = locate_cli() else {
                return CliStatus {
                    found: false,
                    path: None,
                    version: None,
                    agent_installed,
                };
            };
            let version = Command::new(&path)
                .arg("--version")
                .stdin(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
            CliStatus {
                found: true,
                path: Some(path.display().to_string()),
                version,
                agent_installed,
            }
        })
        .await
        .map_err(|error| format!("cli_status task failed: {error}"))
    })
    .await
}

/// Run one job in `workspace`. Needs the CLI to be installed; says so plainly when it is not.
#[tauri::command]
pub async fn cli_run(
    app: AppHandle,
    state: State<'_, CliRun>,
    workspace: String,
    job: CliJob,
) -> Result<JobResult, String> {
    let state = state.inner().clone();
    run_guarded_async("cli_run", async move {
        let cli = locate_cli().ok_or("cli_missing: the athenaeum command is not installed")?;
        // Absolute only: a relative path could be read as a flag, and the window always has the full path.
        if !Path::new(&workspace).is_absolute() || !Path::new(&workspace).is_dir() {
            return Err(format!(
                "invalid_request: {workspace} is not an existing folder given as a full path"
            ));
        }
        let args = job.args(&workspace)?;
        let label = job.label();
        tauri::async_runtime::spawn_blocking(move || {
            let mut command = Command::new(cli);
            command.args(args).env("ATHENAEUM_PROGRESS", "json");
            run_process(&app, &state, command, label, false)
        })
        .await
        .map_err(|error| format!("cli_run task failed: {error}"))?
    })
    .await
}

/// Stop the running job, if there is one (for example a sign-in the person gave up on).
#[tauri::command]
pub fn cli_cancel(state: State<'_, CliRun>) -> bool {
    let child = state.current.lock().ok().and_then(|slot| slot.clone());
    match child {
        Some(child) => child
            .lock()
            .map(|mut child| {
                #[cfg(unix)]
                if Command::new("/bin/kill")
                    .args(["-TERM", &format!("-{}", child.id())])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
                {
                    return true;
                }
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    if Command::new("taskkill.exe")
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .creation_flags(0x0800_0000)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
                child.kill().is_ok()
            })
            .unwrap_or(false),
        None => false,
    }
}

/// Install packaged tools with the bundled Node runtime. No shell bootstrap or
/// system Node; the optional agent delegates to athenaeum_agent's own installer.
#[tauri::command]
pub async fn cli_install_tools(
    app: AppHandle,
    state: State<'_, CliRun>,
    agent: Option<bool>,
) -> Result<JobResult, String> {
    let state = state.inner().clone();
    run_guarded_async("cli_install_tools", async move {
        let dir = installer_dir(&app)?;
        let tools = app.path().resolve("tools", BaseDirectory::Resource)
            .map_err(|e| e.to_string())?;
        let node = tools.join("node").join(if cfg!(windows) { "node.exe" } else { "bin/node" });
        if !node.is_file() {
            return Err("installer_missing: this development build has no packaged tools. Install a native Athenaeum release to use guided setup.".into());
        }
        tauri::async_runtime::spawn_blocking(move || {
            let mut command = Command::new(node);
            command.arg(dir.join("native.mjs"));
            if agent.unwrap_or(false) {
                command.arg("--agent");
            }
            run_process(&app, &state, command, "install", true)
        })
        .await
        .map_err(|error| format!("cli_install_tools task failed: {error}"))?
    })
    .await
}

fn installer_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let bundled = app.path().resolve("install", BaseDirectory::Resource).ok();
    let candidates = bundled.into_iter().chain(
        // `tauri dev` runs from the source tree, where the installer sits beside src-tauri.
        option_env!("CARGO_MANIFEST_DIR")
            .map(|manifest| Path::new(manifest).join("..").join("install")),
    );
    candidates
        .into_iter()
        .find(|dir| dir.join("install.mjs").is_file())
        .ok_or_else(|| {
            "installer_missing: this build of the app does not include the installer".to_string()
        })
}
