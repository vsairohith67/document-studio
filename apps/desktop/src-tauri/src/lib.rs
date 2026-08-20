pub mod app_state;
pub mod contracts;
pub mod database;
pub mod diagnostic_copy;
pub mod diagnostics;
pub mod ipc;
pub mod job_engine;
pub mod operation_registry;
pub mod page_plan;
pub mod path_policy;
pub mod pdf_merge;
pub mod pdf_operations;
pub mod process_sandbox;
pub mod publication;
pub mod qpdf;
pub mod recovery;
pub mod viewer_sessions;
pub mod windows_security;
pub mod workspace;

use app_state::AppState;
use chrono::{DateTime, Utc};
use database::Database;
use recovery::reconcile_startup;
use std::path::Path;
use tauri::{Emitter, Manager};
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

pub fn initialize_runtime_with_resources(
    app_data: &Path,
    resource_directory: &Path,
    maintenance_time: DateTime<Utc>,
) -> Result<AppState, RuntimeInitializationError> {
    let state = initialize_runtime(app_data, maintenance_time)?;
    let bundle = resource_directory.join("resources/qpdf/12.3.2");
    #[cfg(debug_assertions)]
    let bundle = if bundle.is_dir() {
        bundle
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
    };
    Ok(state.with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
        bundle,
        app_data.join("engines"),
    )))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(feature = "test-runtime"))]
    remove_remote_debugging_arguments();
    let builder = tauri::Builder::default()
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("document-studio-navigation-guard")
                .on_navigation(|_, url| {
                    url.scheme() == "tauri"
                        || matches!(
                            (url.scheme(), url.host_str()),
                            ("http" | "https", Some("tauri.localhost"))
                        )
                        || (cfg!(debug_assertions)
                            && url.scheme() == "http"
                            && url.host_str() == Some("localhost")
                            && url.port() == Some(1420))
                })
                .build(),
        )
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
            let resource_directory = app.path().resource_dir()?;
            let state =
                initialize_runtime_with_resources(&app_data, &resource_directory, Utc::now())?;
            #[cfg(feature = "test-runtime")]
            std::fs::write(
                app_data.join(format!("runtime-started-{}", std::process::id())),
                b"setup reached",
            )?;
            app.manage(state);
            Ok(())
        });
    #[cfg(not(feature = "test-runtime"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        ipc::system_status,
        ipc::operations_list,
        ipc::files_inspect,
        ipc::jobs_create,
        ipc::jobs_create_core_pdf,
        ipc::jobs_cancel,
        ipc::jobs_resolve_interrupted,
        ipc::jobs_get,
        ipc::history_list,
        ipc::history_delete,
        ipc::dependencies_scan,
        ipc::settings_get,
        ipc::settings_set,
        ipc::viewer_open_dialog,
        ipc::viewer_read_range,
        ipc::viewer_close,
        ipc::viewer_set_drop_enabled,
        ipc::viewer_choose_destination,
        ipc::viewer_revoke_destination,
    ]);
    #[cfg(feature = "test-runtime")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        ipc::system_status,
        ipc::operations_list,
        ipc::files_inspect,
        ipc::jobs_create,
        ipc::jobs_create_core_pdf,
        ipc::jobs_cancel,
        ipc::jobs_resolve_interrupted,
        ipc::jobs_get,
        ipc::history_list,
        ipc::history_delete,
        ipc::dependencies_scan,
        ipc::settings_get,
        ipc::settings_set,
        ipc::viewer_open_dialog,
        ipc::viewer_open_test_fixture,
        ipc::viewer_read_range,
        ipc::viewer_close,
        ipc::viewer_set_drop_enabled,
        ipc::viewer_choose_destination,
        ipc::viewer_revoke_destination,
    ]);
    builder
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                if !window.state::<AppState>().viewer_sessions.drop_enabled() {
                    return;
                }
                let result = if paths.len() == 1 {
                    window
                        .state::<AppState>()
                        .viewer_sessions
                        .open_pdf(&paths[0])
                } else {
                    Err(contracts::OperationError::safe(
                        "VIEWER_DROP_COUNT_INVALID",
                        "Drop one PDF at a time",
                        "Open a single local PDF in the viewer.",
                        contracts::OperationStage::Inspect,
                        false,
                    ))
                };
                match result {
                    Ok(metadata) => {
                        let _ = window.emit(contracts::VIEWER_DOCUMENT_OPENED_EVENT_NAME, metadata);
                    }
                    Err(error) => {
                        let _ = window.emit("document-studio-viewer-open-failed-v1", error);
                    }
                }
            }
            tauri::WindowEvent::Destroyed => {
                window.state::<AppState>().viewer_sessions.close_all();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Document Studio");
}

#[cfg(not(feature = "test-runtime"))]
fn remove_remote_debugging_arguments() {
    let Some(arguments) = std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") else {
        return;
    };
    let filtered = arguments
        .to_string_lossy()
        .split_whitespace()
        .filter(|argument| {
            !argument.starts_with("--remote-debugging-port")
                && !argument.starts_with("--remote-allow-origins")
        })
        .collect::<Vec<_>>()
        .join(" ");
    if filtered.is_empty() {
        std::env::remove_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS");
    } else {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", filtered);
    }
}

fn runtime_app_data_directory(app: &tauri::App) -> Result<std::path::PathBuf, tauri::Error> {
    #[cfg(feature = "test-runtime")]
    if let Some(path) = std::env::var_os("DOCUMENT_STUDIO_TEST_APP_DATA") {
        return Ok(std::path::PathBuf::from(path));
    }
    app.path().app_data_dir()
}
