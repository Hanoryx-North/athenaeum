//! Athenaeum desktop shell.
//!
//! Deliberately thin. The knowledge model, its storage, navigation and the
//! rules about what may be trusted all live in `athenaeum-core`, which knows
//! nothing about Tauri. This crate opens a window, wires up commands and gets
//! out of the way — which is why the same behaviour is reachable from the
//! CLI, and testable without a display server.

mod commands;
mod panic_guard;

/// Start the application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::open_workspace,
            commands::workspace_path,
            commands::stats,
            commands::list_nodes,
            commands::node_detail,
            commands::create_node,
            commands::update_node,
            commands::set_node_status,
            commands::delete_node,
            commands::link_nodes,
            commands::promote_edge,
            commands::remove_edge,
            commands::attach_evidence,
            commands::search,
            commands::navigate_from,
            commands::assemble_brief,
            commands::journal,
            commands::vocabularies,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Athenaeum");
}
