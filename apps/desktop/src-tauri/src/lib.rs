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

#[cfg(feature = "test-runtime")]
const TEST_WEBVIEW2_DATA_DIRECTORY_ENV: &str = "DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR";
#[cfg(feature = "test-runtime")]
const TEST_WEBVIEW2_CDP_PORT_ENV: &str = "DOCUMENT_STUDIO_TEST_CDP_PORT";
#[cfg(feature = "test-runtime")]
const TEST_APP_DATA_ENV: &str = "DOCUMENT_STUDIO_TEST_APP_DATA";
#[cfg(feature = "test-runtime")]
const WEBVIEW2_REQUIRED_DISABLED_FEATURES: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

#[derive(Debug, Error)]
pub enum RuntimeInitializationError {
    #[error("application data directory could not be prepared")]
    Io(#[from] std::io::Error),
    #[error("metadata runtime could not be initialized")]
    Database(#[from] database::DatabaseError),
    #[error("workspace runtime could not be initialized")]
    Workspace(#[from] workspace::WorkspaceError),
}

#[cfg(feature = "test-runtime")]
#[derive(Debug, Error, PartialEq, Eq)]
enum TestWebviewOverrideError {
    #[error("test WebView2 data directory requires the isolated test app-data boundary")]
    MissingAppDataBoundary,
    #[error("test WebView2 CDP port requires an isolated data directory")]
    PortWithoutDataDirectory,
    #[error("test WebView2 CDP port is invalid")]
    InvalidPort,
    #[error("test WebView2 data directory is invalid")]
    InvalidDataDirectory,
    #[error("test WebView2 data directory is outside the isolated test app-data boundary")]
    DataDirectoryOutsideBoundary,
    #[error("test WebView2 data directory must not use a production profile boundary")]
    DataDirectoryIsProductionProfile,
    #[error("test WebView2 data directory must be a newly created empty directory")]
    DataDirectoryNotEmpty,
    #[error("test WebView2 data directory must not be a reparse point")]
    DataDirectoryIsReparsePoint,
    #[error("the generated Tauri context must contain exactly one main window")]
    MainWindowConfigurationInvalid,
    #[error("the generated Tauri context contains another auto-created window")]
    AdditionalAutoWindow,
    #[error("the isolated test WebView2 main window was not registered exactly once")]
    MainWindowRegistrationInvalid,
}

#[cfg(feature = "test-runtime")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct TestWebviewSettings {
    data_directory: std::path::PathBuf,
    additional_browser_args: Option<String>,
}

#[cfg(feature = "test-runtime")]
#[derive(Clone)]
struct TestWebviewOverride {
    window_config: tauri::utils::config::WindowConfig,
    settings: TestWebviewSettings,
}

#[cfg(feature = "test-runtime")]
fn test_webview_settings_from_environment(
) -> Result<Option<TestWebviewSettings>, TestWebviewOverrideError> {
    validate_test_webview_settings(
        std::env::var_os(TEST_WEBVIEW2_DATA_DIRECTORY_ENV).as_deref(),
        std::env::var_os(TEST_WEBVIEW2_CDP_PORT_ENV).as_deref(),
        std::env::var_os(TEST_APP_DATA_ENV).as_deref(),
    )
}

#[cfg(feature = "test-runtime")]
fn validate_test_webview_settings(
    data_directory: Option<&std::ffi::OsStr>,
    cdp_port: Option<&std::ffi::OsStr>,
    app_data_boundary: Option<&std::ffi::OsStr>,
) -> Result<Option<TestWebviewSettings>, TestWebviewOverrideError> {
    let Some(data_directory) = data_directory else {
        return if cdp_port.is_some() {
            Err(TestWebviewOverrideError::PortWithoutDataDirectory)
        } else {
            Ok(None)
        };
    };
    let Some(app_data_boundary) = app_data_boundary else {
        return Err(TestWebviewOverrideError::MissingAppDataBoundary);
    };

    let requested_data_directory = std::path::PathBuf::from(data_directory);
    let requested_app_data_boundary = std::path::PathBuf::from(app_data_boundary);
    if !requested_data_directory.is_absolute() || !requested_app_data_boundary.is_absolute() {
        return Err(TestWebviewOverrideError::InvalidDataDirectory);
    }

    let app_data_metadata = std::fs::symlink_metadata(&requested_app_data_boundary)
        .map_err(|_| TestWebviewOverrideError::InvalidDataDirectory)?;
    if !app_data_metadata.is_dir() || is_reparse_point(&app_data_metadata) {
        return Err(TestWebviewOverrideError::InvalidDataDirectory);
    }

    let metadata = std::fs::symlink_metadata(&requested_data_directory)
        .map_err(|_| TestWebviewOverrideError::InvalidDataDirectory)?;
    if is_reparse_point(&metadata) {
        return Err(TestWebviewOverrideError::DataDirectoryIsReparsePoint);
    }
    if !metadata.is_dir() {
        return Err(TestWebviewOverrideError::InvalidDataDirectory);
    }
    if std::fs::read_dir(&requested_data_directory)
        .map_err(|_| TestWebviewOverrideError::InvalidDataDirectory)?
        .next()
        .is_some()
    {
        return Err(TestWebviewOverrideError::DataDirectoryNotEmpty);
    }

    let canonical_app_data = std::fs::canonicalize(&requested_app_data_boundary)
        .map_err(|_| TestWebviewOverrideError::InvalidDataDirectory)?;
    let canonical_data_directory = std::fs::canonicalize(&requested_data_directory)
        .map_err(|_| TestWebviewOverrideError::InvalidDataDirectory)?;
    if canonical_data_directory == canonical_app_data
        || !canonical_data_directory.starts_with(&canonical_app_data)
    {
        return Err(TestWebviewOverrideError::DataDirectoryOutsideBoundary);
    }

    let additional_browser_args = cdp_port
        .map(parse_test_cdp_port)
        .transpose()?
        .map(|port| {
            format!(
                "{WEBVIEW2_REQUIRED_DISABLED_FEATURES} --remote-debugging-port={port} --remote-allow-origins=http://127.0.0.1:{port}"
            )
        });
    Ok(Some(TestWebviewSettings {
        data_directory: canonical_data_directory,
        additional_browser_args,
    }))
}

#[cfg(feature = "test-runtime")]
fn parse_test_cdp_port(value: &std::ffi::OsStr) -> Result<u16, TestWebviewOverrideError> {
    let value = value
        .to_str()
        .ok_or(TestWebviewOverrideError::InvalidPort)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TestWebviewOverrideError::InvalidPort);
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| TestWebviewOverrideError::InvalidPort)?;
    if port < 1024 {
        return Err(TestWebviewOverrideError::InvalidPort);
    }
    Ok(port)
}

#[cfg(all(feature = "test-runtime", windows))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(all(feature = "test-runtime", not(windows)))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(feature = "test-runtime")]
fn prepare_test_webview_context(
    context: &mut tauri::Context<tauri::Wry>,
) -> Result<Option<TestWebviewOverride>, TestWebviewOverrideError> {
    let Some(settings) = test_webview_settings_from_environment()? else {
        return Ok(None);
    };
    reject_production_profile_boundary(context, &settings.data_directory)?;
    let windows = &mut context.config_mut().app.windows;
    let main_indices = windows
        .iter()
        .enumerate()
        .filter_map(|(index, window)| (window.label == "main").then_some(index))
        .collect::<Vec<_>>();
    if main_indices.len() != 1 || !windows[main_indices[0]].create {
        return Err(TestWebviewOverrideError::MainWindowConfigurationInvalid);
    }
    if windows
        .iter()
        .enumerate()
        .any(|(index, window)| index != main_indices[0] && window.create)
    {
        return Err(TestWebviewOverrideError::AdditionalAutoWindow);
    }
    let main_index = main_indices[0];
    let window_config = windows[main_index].clone();
    windows[main_index].create = false;
    Ok(Some(TestWebviewOverride {
        window_config,
        settings,
    }))
}

#[cfg(all(feature = "test-runtime", windows))]
fn reject_production_profile_boundary(
    context: &tauri::Context<tauri::Wry>,
    data_directory: &Path,
) -> Result<(), TestWebviewOverrideError> {
    for root in [
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("APPDATA"),
    ]
    .into_iter()
    .flatten()
    {
        let profile = std::path::PathBuf::from(root).join(&context.config().identifier);
        if let Ok(profile) = std::fs::canonicalize(profile) {
            if data_directory == profile || data_directory.starts_with(profile) {
                return Err(TestWebviewOverrideError::DataDirectoryIsProductionProfile);
            }
        }
    }
    Ok(())
}

#[cfg(all(feature = "test-runtime", not(windows)))]
fn reject_production_profile_boundary(
    _context: &tauri::Context<tauri::Wry>,
    _data_directory: &Path,
) -> Result<(), TestWebviewOverrideError> {
    Ok(())
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
    let context = tauri::generate_context!();
    #[cfg(feature = "test-runtime")]
    let mut context = context;
    #[cfg(feature = "test-runtime")]
    let test_webview_override = prepare_test_webview_context(&mut context)
        .expect("the isolated test WebView2 window configuration is invalid");
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
        .setup(move |app| {
            let app_data = runtime_app_data_directory(app)?;
            let resource_directory = app.path().resource_dir()?;
            let state =
                initialize_runtime_with_resources(&app_data, &resource_directory, Utc::now())?;
            app.manage(state);
            #[cfg(feature = "test-runtime")]
            if let Some(test_webview_override) = &test_webview_override {
                let mut window_builder = tauri::WebviewWindowBuilder::from_config(
                    app,
                    &test_webview_override.window_config,
                )?
                .data_directory(test_webview_override.settings.data_directory.clone());
                if let Some(arguments) = &test_webview_override.settings.additional_browser_args {
                    window_builder = window_builder.additional_browser_args(arguments);
                }
                let window = window_builder.build()?;
                let registered_windows = app.webview_windows();
                if window.label() != "main"
                    || registered_windows.len() != 1
                    || !registered_windows.contains_key("main")
                {
                    return Err(TestWebviewOverrideError::MainWindowRegistrationInvalid.into());
                }
            }
            #[cfg(feature = "test-runtime")]
            std::fs::write(
                app_data.join(format!("runtime-started-{}", std::process::id())),
                b"setup reached",
            )?;
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
        .run(context)
        .expect("error while running Document Studio");
}

#[cfg(not(feature = "test-runtime"))]
fn remove_remote_debugging_arguments() {
    let Some(arguments) = std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") else {
        return;
    };
    if sanitize_inherited_webview_arguments(&arguments.to_string_lossy()).is_none() {
        std::env::remove_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS");
    }
}

#[cfg(any(not(feature = "test-runtime"), test))]
fn sanitize_inherited_webview_arguments(arguments: &str) -> Option<&str> {
    let lowercase = arguments.to_ascii_lowercase();
    let malformed_quotes = arguments
        .chars()
        .filter(|character| *character == '"')
        .count()
        % 2
        != 0;
    if malformed_quotes
        || lowercase.contains("--remote-debugging-")
        || lowercase.contains("--remote-allow-origins")
    {
        None
    } else {
        Some(arguments)
    }
}

fn runtime_app_data_directory(app: &tauri::App) -> Result<std::path::PathBuf, tauri::Error> {
    #[cfg(feature = "test-runtime")]
    if let Some(path) = std::env::var_os("DOCUMENT_STUDIO_TEST_APP_DATA") {
        return Ok(std::path::PathBuf::from(path));
    }
    app.path().app_data_dir()
}

#[cfg(all(test, feature = "test-runtime"))]
mod test_webview_override_tests {
    use super::*;
    use std::ffi::OsStr;

    fn isolated_directories() -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().expect("temporary root");
        let app_data = root.path().join("app-data");
        let webview_data = app_data.join("webview2-user-data");
        std::fs::create_dir_all(&webview_data).expect("empty WebView2 data directory");
        (root, webview_data)
    }

    #[test]
    fn neither_variable_preserves_automatic_window_mode() {
        assert_eq!(validate_test_webview_settings(None, None, None), Ok(None));
    }

    #[test]
    fn valid_directory_and_port_build_exact_loopback_arguments() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        let settings = validate_test_webview_settings(
            Some(webview_data.as_os_str()),
            Some(OsStr::new("43127")),
            Some(app_data.as_os_str()),
        )
        .expect("valid settings")
        .expect("manual window mode");
        assert_eq!(
            settings.additional_browser_args.as_deref(),
            Some("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port=43127 --remote-allow-origins=http://127.0.0.1:43127")
        );
        assert_eq!(
            settings.data_directory,
            std::fs::canonicalize(webview_data).expect("canonical test data directory")
        );
    }

    #[test]
    fn valid_directory_without_port_builds_manual_window_without_cdp() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        let settings = validate_test_webview_settings(
            Some(webview_data.as_os_str()),
            None,
            Some(app_data.as_os_str()),
        )
        .expect("valid no-CDP settings")
        .expect("manual window mode");
        assert_eq!(settings.additional_browser_args, None);
    }

    #[test]
    fn port_without_data_directory_is_rejected() {
        assert_eq!(
            validate_test_webview_settings(None, Some(OsStr::new("43127")), None),
            Err(TestWebviewOverrideError::PortWithoutDataDirectory)
        );
    }

    #[test]
    fn invalid_ports_are_rejected() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        for value in ["1023", "65536", "43127,43128", " 43127", "43127 ", "abc"] {
            assert_eq!(
                validate_test_webview_settings(
                    Some(webview_data.as_os_str()),
                    Some(OsStr::new(value)),
                    Some(app_data.as_os_str()),
                ),
                Err(TestWebviewOverrideError::InvalidPort),
                "port value {value:?} must fail closed"
            );
        }
    }

    #[test]
    fn relative_and_out_of_boundary_directories_are_rejected() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        assert_eq!(
            validate_test_webview_settings(
                Some(OsStr::new("relative-webview-data")),
                None,
                Some(app_data.as_os_str()),
            ),
            Err(TestWebviewOverrideError::InvalidDataDirectory)
        );
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).expect("outside directory");
        assert_eq!(
            validate_test_webview_settings(
                Some(outside.as_os_str()),
                None,
                Some(app_data.as_os_str()),
            ),
            Err(TestWebviewOverrideError::DataDirectoryOutsideBoundary)
        );
        assert!(webview_data.is_dir());
    }

    #[test]
    fn nonempty_and_reparse_directories_are_rejected() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        std::fs::write(webview_data.join("unexpected"), b"x").expect("nonempty marker");
        assert_eq!(
            validate_test_webview_settings(
                Some(webview_data.as_os_str()),
                None,
                Some(app_data.as_os_str()),
            ),
            Err(TestWebviewOverrideError::DataDirectoryNotEmpty)
        );

        #[cfg(windows)]
        {
            let target = app_data.join("junction-target");
            let junction_path = app_data.join("junction-data");
            std::fs::create_dir(&target).expect("junction target");
            junction::create(&target, &junction_path).expect("test junction");
            assert_eq!(
                validate_test_webview_settings(
                    Some(junction_path.as_os_str()),
                    None,
                    Some(app_data.as_os_str()),
                ),
                Err(TestWebviewOverrideError::DataDirectoryIsReparsePoint)
            );
            junction::delete(&junction_path).expect("remove test junction");
        }
    }

    #[test]
    fn generated_arguments_exclude_unsafe_and_profile_switches() {
        let (root, webview_data) = isolated_directories();
        let app_data = root.path().join("app-data");
        let arguments = validate_test_webview_settings(
            Some(webview_data.as_os_str()),
            Some(OsStr::new("43127")),
            Some(app_data.as_os_str()),
        )
        .expect("valid settings")
        .expect("manual window mode")
        .additional_browser_args
        .expect("CDP arguments");
        for forbidden in [
            "*",
            "--user-data-dir",
            "unsafe-eval",
            "0.0.0.0",
            "localhost",
        ] {
            assert!(
                !arguments.contains(forbidden),
                "arguments must not contain {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod inherited_webview_argument_tests {
    use super::sanitize_inherited_webview_arguments;

    #[test]
    fn benign_arguments_are_preserved_byte_for_byte() {
        let value = "--disable-gpu --force-color-profile=srgb";
        assert_eq!(sanitize_inherited_webview_arguments(value), Some(value));
    }

    #[test]
    fn every_remote_debugging_family_variant_clears_the_entire_value() {
        for value in [
            "--remote-debugging-port=43127",
            "--remote-debugging-port 43127",
            "--remote-debugging-pipe",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-new-variant=yes",
            "\"--remote-debugging-port=43127\"",
            "--REMOTE-DEBUGGING-PORT=43127",
            "--Remote-Debugging-Pipe --disable-gpu",
            "--remote-allow-origins=*",
            "--remote-allow-origins http://127.0.0.1:43127",
            "--disable-gpu --remote-debugging-pipe",
        ] {
            assert_eq!(
                sanitize_inherited_webview_arguments(value),
                None,
                "dangerous value must fail closed: {value}"
            );
        }
    }

    #[test]
    fn malformed_quoting_fails_closed() {
        assert_eq!(
            sanitize_inherited_webview_arguments("--disable-gpu \"unterminated"),
            None
        );
    }
}
