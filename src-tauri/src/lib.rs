//! Athenaeum desktop shell.
//!
//! Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
//! Modified for Athenaeum.
//!
//! The knowledge model, its storage, navigation and the rules about what may be
//! trusted live in `athenaeum-core`, which knows nothing about Tauri. This crate
//! opens a window, wires up commands and gets out of the way — which is why the
//! same behaviour is reachable from the CLI, and testable without a display server.
//!
//! **No agent runtime here.** llm_wiki ran a coding agent in this process; Athenaeum
//! does not. The agent loop, its tools, permissions and sessions belong to DeepSeek
//! Harness, reached through `athenaeum_agent` over the `athenaeum bridge` protocol.
//! See `AGENTS.md` and `docs/porting-athenaeum.md`.

mod commands;
mod core_bridge;
mod engine;
mod harness;
mod knowledge;
mod panic_guard;
mod proxy;
mod search_backends;
mod tray;
mod types;

use std::sync::Mutex;
use tauri::Manager;

struct CloseBehaviorState(Mutex<String>);
struct TrayAvailabilityState(Mutex<bool>);

#[tauri::command]
fn set_proxy_env(config: proxy::ProxyConfig) -> String {
    let summary = proxy::apply_proxy_env(&config);
    eprintln!("[proxy] live update: {summary}");
    summary
}

#[tauri::command]
fn set_close_behavior(
    value: String,
    state: tauri::State<'_, CloseBehaviorState>,
) -> Result<String, String> {
    let normalized = match value.as_str() {
        "ask" | "minimize" | "exit" => value,
        other => return Err(format!("Invalid close behavior: {other}")),
    };
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "Close behavior state is unavailable".to_string())?;
    *guard = normalized.clone();
    Ok(normalized)
}

fn close_behavior<R: tauri::Runtime>(window: &tauri::Window<R>) -> String {
    window
        .state::<CloseBehaviorState>()
        .0
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| "minimize".to_string())
}

fn tray_available<R: tauri::Runtime>(window: &tauri::Window<R>) -> bool {
    window
        .state::<TrayAvailabilityState>()
        .0
        .lock()
        .map(|value| *value)
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    apply_linux_webkit_compat_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        // Rust-backed fetch so third-party LLM APIs that reject
        // browser-origin headers via CORS preflight (MiniMax, Volcengine
        // Ark's api/coding/v3, etc.) still work. Requests leave the app
        // from Rust, never the webview.
        .plugin(tauri_plugin_http::init())
        // In-app updates: the updater verifies a minisign signature against
        // the public key in tauri.conf.json before installing anything, and
        // the process plugin restarts the app into the new version.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Let the PDF extractor find the bundled pdfium dynamic
            // library via Tauri's platform-correct resource path.
            if let Ok(dir) = app.path().resource_dir() {
                commands::fs::set_resource_dir_hint(dir);
            }
            // Apply user-configured global HTTP proxy by setting
            // HTTP_PROXY / HTTPS_PROXY / NO_PROXY env vars BEFORE
            // any HTTP request is made. tauri-plugin-http's reqwest
            // client reads these on first construction. Lives next
            // to the resource-dir hint so the proxy applies to
            // everything: LLM, embedding, update check, deep
            // research, captioning. See src-tauri/src/proxy.rs.
            if let Ok(dir) = app.path().app_data_dir() {
                let store_path = dir.join("app-state.json");
                eprintln!("[proxy] reading from {}", store_path.display());
                if let Some(cfg) = proxy::read_proxy_config_from_store(&store_path) {
                    let summary = proxy::apply_proxy_env(&cfg);
                    eprintln!("[proxy] {summary}");
                } else {
                    eprintln!("[proxy] no proxyConfig in store, requests go direct");
                }
            } else {
                eprintln!("[proxy] could not resolve app_data_dir");
            }
            app.manage(commands::file_sync::FileSyncState::default());
            app.manage(harness::HarnessChatState::default());
            app.manage(knowledge::AppState::default());
            app.manage(CloseBehaviorState(Mutex::new("minimize".to_string())));
            app.manage(TrayAvailabilityState(Mutex::new(false)));
            let tray_available = match tray::create_tray(app.handle()) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("[tray] system tray unavailable, continuing without it: {err}");
                    false
                }
            };
            match app.state::<TrayAvailabilityState>().0.lock() {
                Ok(mut state) => {
                    *state = tray_available;
                }
                Err(err) => {
                    eprintln!("[tray] failed to update tray availability state: {err}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::fs::read_file,
            commands::fs::write_file,
            commands::fs::write_file_base64,
            commands::fs::write_file_atomic,
            commands::fs::apply_text_selection_edit,
            commands::fs::create_missing_wiki_page,
            commands::file_history::list_file_history,
            commands::file_history::restore_file_history,
            commands::file_history::get_file_history_stats,
            commands::file_history::get_file_history_settings,
            commands::file_history::set_file_history_settings,
            commands::file_history::clear_file_history,
            commands::fs::list_directory,
            commands::fs::copy_file,
            commands::fs::copy_directory,
            commands::fs::preprocess_file,
            commands::fs::delete_file,
            commands::fs::find_related_wiki_pages,
            commands::fs::create_directory,
            commands::fs::file_exists,
            commands::fs::get_file_modified_time,
            commands::fs::get_file_size,
            commands::fs::get_file_md5,
            commands::fs::read_file_as_base64,
            commands::project::create_project,
            commands::project::open_project,
            commands::project::open_project_folder,
            commands::project::open_path_in_project,
            commands::project_maintenance::export_project_archive,
            commands::project_maintenance::import_project_archive,
            commands::project_maintenance::rebuild_wiki_index,
            commands::search::search_project,
            commands::search::embedding_fetch,
            commands::search::embedding_fetch_batch,
            commands::search::get_page_links,
            commands::external_search::web_search,
            commands::external_search::anytxt_search,
            commands::vectorstore::vector_upsert,
            commands::vectorstore::vector_search,
            commands::vectorstore::vector_delete,
            commands::vectorstore::vector_count,
            commands::vectorstore::vector_upsert_chunks,
            commands::vectorstore::vector_search_chunks,
            commands::vectorstore::vector_delete_page,
            commands::vectorstore::vector_count_chunks,
            commands::vectorstore::vector_clear_chunks,
            commands::vectorstore::vector_optimize_chunks,
            commands::vectorstore::vector_legacy_row_count,
            commands::vectorstore::vector_drop_legacy,
            commands::extract_images::extract_pdf_images_cmd,
            commands::extract_images::extract_office_images_cmd,
            commands::extract_images::extract_and_save_pdf_images_cmd,
            commands::extract_images::extract_and_save_office_images_cmd,
            commands::file_sync::start_project_file_watcher,
            commands::file_sync::stop_project_file_watcher,
            commands::file_sync::rescan_project_files,
            commands::file_sync::get_file_change_queue,
            commands::file_sync::retry_file_change_task,
            commands::file_sync::ignore_file_change_task,
            knowledge::open_workspace,
            knowledge::workspace_path,
            knowledge::stats,
            knowledge::list_nodes,
            knowledge::node_detail,
            knowledge::create_node,
            knowledge::update_node,
            knowledge::set_node_status,
            knowledge::delete_node,
            knowledge::link_nodes,
            knowledge::promote_edge,
            knowledge::remove_edge,
            knowledge::attach_evidence,
            knowledge::search,
            knowledge::navigate_from,
            knowledge::assemble_brief,
            knowledge::journal,
            knowledge::index_vault,
            knowledge::knowledge_graph,
            knowledge::lint_vault,
            knowledge::vocabularies,
            core_bridge::core_bridge,
            engine::engine_request,
            engine::engine_accept,
            engine::engine_reject,
            engine::library_link,
            harness::harness_check,
            harness::harness_start_turn_stream,
            harness::harness_cancel_turn,
            set_proxy_env,
            set_close_behavior,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let behavior = close_behavior(window);
                let win = window.clone();
                let app = window.app_handle().clone();
                match behavior.as_str() {
                    "exit" => {
                        tauri::async_runtime::spawn(async move {
                            let _ = win.destroy();
                            app.exit(0);
                        });
                    }
                    "minimize" => {
                        if tray_available(window) {
                            let _ = window.hide();
                        } else {
                            let _ = window.minimize();
                        }
                    }
                    _ => {
                        tauri::async_runtime::spawn(async move {
                            use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
                            let confirmed = app
                                .dialog()
                                .message(
                                    "Quit Athenaeum? Choose Quit to exit. Choose Hide Window to keep background features running.",
                                )
                                .title("Athenaeum")
                                .buttons(MessageDialogButtons::OkCancelCustom(
                                    "Quit".to_string(),
                                    "Hide Window".to_string(),
                                ))
                                .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                                .blocking_show();

                            if confirmed {
                                let _ = win.destroy();
                                app.exit(0);
                            } else {
                                let _ = win.hide();
                            }
                        });
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(&event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<harness::HarnessChatState>() {
                    state.shutdown();
                }
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
            {
                if !has_visible_windows {
                    use tauri::Manager;
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            let _ = (app, event); // suppress unused warnings on non-macOS
        });
}

#[cfg(target_os = "linux")]
fn apply_linux_webkit_compat_env() {
    // WebKitGTK can crash or withdraw its window on some Wayland/XWayland
    // stacks unless accelerated render paths are disabled before the WebView
    // is created. Keep these as Linux-only defaults and do not override an
    // explicit user setting so advanced users and packagers can opt back into
    // the platform default if their stack supports it.
    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
    // Some WebKitGTK/Mesa combinations still attempt the DMA-BUF renderer
    // even with accelerated compositing disabled. In an AppImage running
    // through XWayland that can withdraw the native window while leaving the
    // web and network processes alive. Respect explicit packager overrides.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_webkit_compat_env() {}
