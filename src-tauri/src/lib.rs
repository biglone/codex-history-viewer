pub mod db;
pub mod commands;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_version,
            commands::get_sessions,
            commands::get_session_count,
            commands::get_session_messages,
            commands::search_sessions,
            commands::get_projects,
            commands::get_sessions_by_project,
            // Sync commands
            commands::get_sync_config,
            commands::save_sync_config,
            commands::get_local_session_ids,
            commands::get_local_project_paths,
            commands::get_session_for_upload,
            commands::get_sessions_for_upload,
            commands::import_sessions,
            commands::preview_agy_import,
            commands::import_agy_sessions,
            // Automations sync commands
            commands::get_local_automations,
            commands::import_automations,
            commands::debug_automations_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
