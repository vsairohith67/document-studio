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
use chrono::{DateTime, Utc};
use database::Database;
use recovery::reconcile_startup;
use std::path::Path;
use tauri::Manager;
use thiserror::Error;
use workspace::WorkspaceManager;

#[derive(Debug, Error)]
pub enum RuntimeInitializationError {
    #[error("application data directory could not be prepared")]
    Io(#[from] std::io::Error),
    #[error("metadata runtime could not be initialized")]
    Database(#[from] database::DatabaseError),
    #[error("workspace runtime could not be initialized")]
    Workspace(#[from] workspace::WorkspaceError),
}

pub fn initialize_runtime(
    app_data: &Path,
    maintenance_time: DateTime<Utc>,
) -> Result<AppState, RuntimeInitializationError> {
    std::fs::create_dir_all(app_data)?;
    let database = Database::open(&app_data.join("metadata.sqlite3"))?;
    let workspaces = WorkspaceManager::initialize(app_data)?;
    let state = AppState::new(database, workspaces);
    reconcile_startup(&state)?;
    state.database().run_retention_at(maintenance_time)?;
    Ok(state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _forwarded_arguments, _forwarded_working_directory| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = runtime_app_data_directory(app)?;
            let state = initialize_runtime(&app_data, Utc::now())?;
            #[cfg(feature = "test-runtime")]
            std::fs::write(
                app_data.join(format!("runtime-started-{}", std::process::id())),
                b"setup reached",
            )?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::system_status,
            ipc::operations_list,
            ipc::files_inspect,
            ipc::jobs_create,
            ipc::jobs_cancel,
            ipc::jobs_resolve_interrupted,
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

fn runtime_app_data_directory(app: &tauri::App) -> Result<std::path::PathBuf, tauri::Error> {
    #[cfg(feature = "test-runtime")]
    if let Some(path) = std::env::var_os("DOCUMENT_STUDIO_TEST_APP_DATA") {
        return Ok(std::path::PathBuf::from(path));
    }
    app.path().app_data_dir()
}
