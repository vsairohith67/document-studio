pub mod app_state;
pub mod contracts;
pub mod database;
pub mod diagnostic_copy;
pub mod diagnostics;
pub mod ipc;
pub mod job_engine;
pub mod path_policy;
pub mod publication;
pub mod recovery;
pub mod windows_security;
pub mod workspace;

use app_state::AppState;
use database::Database;
use recovery::reconcile_startup;
use tauri::Manager;
use workspace::WorkspaceManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let database = Database::open(&app_data.join("metadata.sqlite3"))?;
            let workspaces = WorkspaceManager::initialize(&app_data)?;
            let state = AppState::new(database, workspaces);
            reconcile_startup(&state)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::system_status,
            ipc::operations_list,
            ipc::files_inspect,
            ipc::jobs_create,
            ipc::jobs_cancel,
            ipc::jobs_get,
            ipc::history_list,
            ipc::history_delete,
            ipc::dependencies_scan,
            ipc::settings_get,
            ipc::settings_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Document Studio");
}
