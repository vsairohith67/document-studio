mod support;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use document_studio_lib::app_state::{AppState, CancelOutcome};
use document_studio_lib::contracts::{JobState, JobsCreateRequest, OperationStage, ProgressEvent};
use document_studio_lib::database::Database;
use document_studio_lib::diagnostic_copy::{DiagnosticCopyHooks, DiagnosticCopyService};
use document_studio_lib::workspace::WorkspaceManager;
use tempfile::tempdir;

fn service() -> (tempfile::TempDir, AppState, DiagnosticCopyService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces);
    let service = DiagnosticCopyService::new(state.clone());
    (app_data, state, service)
}

fn request(input: &std::path::Path, destination: &std::path::Path) -> JobsCreateRequest {
    JobsCreateRequest {
        operation_id: "diagnostic.copy".to_owned(),
        input_paths: vec![input.to_string_lossy().into_owned()],
        destination_directory: destination.to_string_lossy().into_owned(),
        requested_output_name: "fixture-copy.bin".to_owned(),
    }
}

#[test]
fn diagnostic_copy_completes_full_verified_lifecycle_and_cleans_workspace() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", b"verified foundation copy");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::<ProgressEvent>::new()));
    let captured = events.clone();

    let completed = service
        .execute(&job.id, move |event| captured.borrow_mut().push(event))
        .unwrap();

    assert_eq!(completed.state, JobState::Completed);
    let output = completed.outputs.first().unwrap();
    assert_eq!(output.status.as_str(), "published");
    assert_eq!(
        fs::read(output.final_path.as_ref().unwrap()).unwrap(),
        b"verified foundation copy"
    );
    assert!(!state.workspaces.root().join(&job.id).exists());
    assert!(destination.path().read_dir().unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".partial")));

    let events = events.borrow();
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    for expected in [
        JobState::Inspecting,
        JobState::Preflight,
        JobState::Ready,
        JobState::Running,
        JobState::Verifying,
        JobState::Publishing,
        JobState::Completed,
    ] {
        assert!(events.iter().any(|event| event.state == expected));
    }
    assert!(events
        .iter()
        .all(|event| !event.message.contains("fixture.bin")));
}

#[test]
fn running_cancellation_is_truthful_and_leaves_no_output_or_partial() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let bytes = vec![0x7c; 4 * 1024 * 1024];
    let input = support::write_fixture(source.path(), "fixture.bin", &bytes);
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let cancel_state = state.clone();
    let job_id = job.id.clone();
    let requested = Rc::new(CellFlag::default());
    let requested_in_callback = requested.clone();

    let cancelled = service
        .execute(&job.id, move |event| {
            if event.state == JobState::Running
                && event.stage == OperationStage::Execute
                && event.completed_units > 0
                && !requested_in_callback.replace(true)
            {
                assert_eq!(
                    cancel_state.cancellations.request(&job_id),
                    CancelOutcome::Requested
                );
            }
        })
        .unwrap();

    assert!(requested.get());
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled.outputs[0].final_path.is_none());
    assert!(destination.path().read_dir().unwrap().next().is_none());
    assert!(!state.workspaces.root().join(&job.id).exists());
}

#[test]
fn destination_publication_cancellation_reaches_cancelled_without_a_final_output() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let bytes = vec![0x4d; 8 * 1024 * 1024];
    let input = support::write_fixture(source.path(), "fixture.bin", &bytes);
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let cancel_state = state.clone();
    let job_id = job.id.clone();
    let requested = Rc::new(CellFlag::default());
    let requested_in_callback = requested.clone();

    let cancelled = service
        .execute(&job.id, move |event| {
            if event.state == JobState::Verifying
                && event.stage == OperationStage::Publish
                && event.completed_units > 0
                && !requested_in_callback.replace(true)
            {
                assert_eq!(
                    cancel_state.cancellations.request(&job_id),
                    CancelOutcome::Requested
                );
            }
        })
        .unwrap();

    assert!(requested.get());
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled.outputs[0].final_path.is_none());
    assert!(destination.path().read_dir().unwrap().next().is_none());
    assert!(!state.workspaces.root().join(&job.id).exists());
}

#[derive(Default)]
struct CellFlag(std::cell::Cell<bool>);

impl CellFlag {
    fn get(&self) -> bool {
        self.0.get()
    }

    fn replace(&self, value: bool) -> bool {
        self.0.replace(value)
    }
}

#[test]
fn injected_write_failure_records_failed_and_exact_cleanup() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", &vec![1; 2 * 1024 * 1024]);
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();

    let error = service
        .execute_with_hooks(
            &job.id,
            |_event| {},
            DiagnosticCopyHooks {
                fail_write_after_bytes: Some(0),
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "WRITE_FAILED");
    let failed = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.errors[0].code, "WRITE_FAILED");
    assert!(!state.workspaces.root().join(&job.id).exists());
    assert!(destination.path().read_dir().unwrap().next().is_none());
}

#[test]
fn verification_mismatch_prevents_publication_and_completion() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", b"must verify");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();

    let error = service
        .execute_with_hooks(
            &job.id,
            |_event| {},
            DiagnosticCopyHooks {
                corrupt_staging_before_verify: true,
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "VERIFICATION_MISMATCH");
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
    assert!(destination.path().read_dir().unwrap().next().is_none());
}

#[test]
fn cleanup_failure_after_publication_never_reports_completion() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", b"published but not clean");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();

    let error = service
        .execute_with_hooks(
            &job.id,
            |_event| {},
            DiagnosticCopyHooks {
                fail_cleanup: true,
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "CLEANUP_FAILED");
    let interrupted = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    assert!(
        interrupted.outputs[0]
            .final_path
            .as_ref()
            .unwrap()
            .as_str()
            .len()
            > 3
    );
    assert!(state.workspaces.root().join(&job.id).exists());
}

#[test]
fn collision_created_at_commit_is_preserved_and_retries_the_next_suffix() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", b"verified source");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();

    let completed = service
        .execute_with_hooks(
            &job.id,
            |_event| {},
            DiagnosticCopyHooks {
                create_collision_before_first_publication_commit: true,
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap();

    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        fs::read(destination.path().join("fixture-copy.bin")).unwrap(),
        b"competing output"
    );
    assert_eq!(
        completed.outputs[0].resolved_name.as_deref(),
        Some("fixture-copy (1).bin")
    );
    assert_eq!(
        fs::read(completed.outputs[0].final_path.as_ref().unwrap()).unwrap(),
        b"verified source"
    );
}

#[test]
fn collision_partial_deletion_failure_interrupts_with_exact_ownership_retained() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "fixture.bin", b"cleanup collision");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();

    let error = service
        .execute_with_hooks(
            &job.id,
            |_event| {},
            DiagnosticCopyHooks {
                create_collision_before_first_publication_commit: true,
                lock_partial_before_first_publication_commit: true,
                fail_cleanup: true,
                ..DiagnosticCopyHooks::default()
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "CLEANUP_FAILED");
    let interrupted = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    let partial_path = interrupted.outputs[0].partial_path.as_ref().unwrap();
    assert!(std::path::Path::new(partial_path).exists());
    assert_eq!(
        fs::read(destination.path().join("fixture-copy.bin")).unwrap(),
        b"competing output"
    );
    assert_eq!(
        interrupted
            .errors
            .iter()
            .filter(|operation_error| operation_error.code == "CLEANUP_FAILED")
            .count(),
        1
    );
}

#[test]
fn zero_byte_file_is_valid_and_uses_standard_empty_sha256() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "empty.bin", b"");
    let job = service
        .create_job(request(&input, destination.path()))
        .unwrap();
    let completed = service.execute(&job.id, |_event| {}).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        completed.outputs[0].sha256.as_deref(),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}
