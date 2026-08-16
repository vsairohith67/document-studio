mod support;

use std::fs;

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::{DependencyStatus, JobsCreateRequest, ProgressEvent};
use document_studio_lib::database::Database;
use document_studio_lib::diagnostic_copy::{DiagnosticCopyHooks, DiagnosticCopyService};
use document_studio_lib::diagnostics::scan_dependencies;
use document_studio_lib::workspace::WorkspaceManager;
use tempfile::tempdir;

const CONTENT_SENTINEL: &str = "DOCUMENT_CONTENT_SENTINEL_5fce0d";
const PASSWORD_SENTINEL: &str = "PASSWORD_SENTINEL_9183";

fn request(input: &std::path::Path, destination: &std::path::Path) -> JobsCreateRequest {
    JobsCreateRequest {
        operation_id: "diagnostic.copy".to_owned(),
        input_paths: vec![input.to_string_lossy().into_owned()],
        destination_directory: destination.to_string_lossy().into_owned(),
        requested_output_name: "safe-copy.bin".to_owned(),
    }
}

#[test]
fn document_content_and_secrets_do_not_enter_metadata_events_errors_or_diagnostics() {
    let app_data = tempdir().unwrap();
    let database_path = app_data.path().join("metadata.sqlite3");
    let database = Database::open(&database_path).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces);
    let service = DiagnosticCopyService::new(state.clone());
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(
        source.path(),
        "neutral.bin",
        format!("{CONTENT_SENTINEL}\n{PASSWORD_SENTINEL}").as_bytes(),
    );
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let mut events = Vec::<ProgressEvent>::new();
    service
        .execute(&job.id, |event| events.push(event))
        .unwrap();

    let failed_job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let error = service
        .execute_with_hooks(
            &failed_job.id,
            |_event| {},
            DiagnosticCopyHooks {
                fail_write_after_bytes: Some(0),
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap_err();
    assert!(!error.detail.contains(CONTENT_SENTINEL));
    assert!(!error.detail.contains(PASSWORD_SENTINEL));

    let diagnostics = scan_dependencies(&mut state.database()).unwrap();
    assert!(diagnostics
        .iter()
        .filter(|dependency| {
            matches!(
                dependency.id.as_str(),
                "qpdf" | "pdfjs" | "libreoffice" | "ocrmypdf" | "tesseract"
            )
        })
        .all(|dependency| dependency.status == DependencyStatus::NotRequired));
    assert!(diagnostics.iter().all(|dependency| {
        dependency.version.as_deref() != Some(CONTENT_SENTINEL)
            && dependency.error_code.as_deref() != Some(PASSWORD_SENTINEL)
    }));

    let event_json = serde_json::to_string(&events).unwrap();
    assert!(!event_json.contains(CONTENT_SENTINEL));
    assert!(!event_json.contains(PASSWORD_SENTINEL));

    state
        .database()
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    let database_bytes = fs::read(&database_path).unwrap();
    for sentinel in [CONTENT_SENTINEL, PASSWORD_SENTINEL] {
        assert!(!database_bytes
            .windows(sentinel.len())
            .any(|window| window == sentinel.as_bytes()));
    }
}
