// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Octopus Chat's Tauri shell. The window, the command surface and the event
// bridge live here; storage, providers, tools and the agent loop live in the
// `octopus-core` crate so they can be tested without loading a webview.

mod commands;
mod runs;
mod state;

use std::sync::Arc;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
.plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            app.manage(state::AppState::new(state::open_store()?));
            app.manage(Arc::new(runs::Runs::default()));

            // The webview is created before the window has its final size, and on
            // Windows it then keeps that stale viewport: everything lays out at
            // the screen's width and the right panel and composer render
            // off-screen. Setting the size once here forces a resize, after which
            // the webview tracks the window normally.
            if let Some(window) = app.get_webview_window("main") {
                use tauri::LogicalSize;
                let _ = window.set_size(LogicalSize::new(1280.0, 820.0));
                let _ = window.center();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // A closed window ends any in-flight run so no orphaned agent keeps
            // writing files after the user expected the app to be gone.
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(runs) = window.try_state::<Arc<runs::Runs>>() {
                    runs.cancel_all();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
      commands::pick_project_dirs,
      commands::validate_project_dir,
            commands::settings_get,
            commands::settings_save,
            commands::projects_list,
            commands::project_save,
            commands::project_delete,
            commands::workspace_inspect,
            commands::instructions_scaffold,
            commands::sessions_list,
            commands::session_get,
            commands::session_create,
            commands::session_update,
            commands::session_delete,
            commands::session_rewind,
            commands::messages_list,
            commands::providers_list,
            commands::provider_set_key,
            commands::provider_save_custom,
            commands::provider_delete_custom,
            commands::provider_models,
            commands::provider_probe_models,
            commands::provider_ping,
            commands::model_add,
            commands::rules_list,
            commands::rules_save,
            commands::send_message,
            commands::approve,
            commands::abort_run,
            commands::session_is_running,
            commands::todos_save,
            commands::checkpoint_create,
            commands::checkpoint_list,
            commands::git_state,
            commands::storage_info,
            commands::app_paths,
            commands::set_local_only,
            commands::panel_tree,
            commands::panel_read,
            commands::panel_write,
            commands::panel_changed,
            commands::panel_read_base64,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Octopus Chat");
}