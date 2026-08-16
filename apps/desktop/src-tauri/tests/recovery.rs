mod support;

use std::fs;

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::{JobState, JobsCreateRequest, OperationStage};
use document_studio_lib::database::{Database, DatabaseError};
use document_studio_lib::diagnostic_copy::DiagnosticCopyService;
use document_studio_lib::publication::hash_file;
use document_studio_lib::recovery::reconcile_startup;
use document_studio_lib::workspace::WorkspaceManager;
use tempfile::tempdir;

fn setup() -> (tempfile::TempDir, AppState, DiagnosticCopyService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces);
    let service = DiagnosticCopyService::new(state.clone());
    (app_data, state, service)
}

fn create_job(
    service: &DiagnosticCopyService,
    input: &std::path::Path,
    destination: &std::path::Path,
) -> document_studio_lib::contracts::JobRecord {
    service
        .create_job(JobsCreateRequest {
            operation_id: "diagnostic.copy".to_owned(),
            input_paths: vec![input.to_string_lossy().into_owned()],
            destination_directory: destination.to_string_lossy().into_owned(),
            requested_output_name: "recovery-copy.bin".to_owned(),
        })
        .unwrap()
}

fn advance(
    state: &AppState,
    job_id: &str,
    from: JobState,
    to: JobState,
    stage: OperationStage,
) -> Result<(), DatabaseError> {
    let mut database = state.database();
    let current = database.get_job(job_id)?.unwrap();
    database.transition_job(job_id, from, current.version, to, Some(stage))?;
    Ok(())
}

fn advance_to_running(state: &AppState, job_id: &str) {
    advance(
        state,
        job_id,
        JobState::Queued,
        JobState::Inspecting,
        OperationStage::Inspect,
    )
    .unwrap();
    advance(
        state,
        job_id,
        JobState::Inspecting,
        JobState::Preflight,
        OperationStage::Preflight,
    )
    .unwrap();
    advance(
        state,
        job_id,
        JobState::Preflight,
        JobState::Ready,
        OperationStage::Plan,
    )
    .unwrap();
    advance(
        state,
        job_id,
        JobState::Ready,
        JobState::Running,
        OperationStage::Execute,
    )
    .unwrap();
}

#[test]
fn startup_marks_running_job_interrupted_and_removes_owned_workspace() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"crash fixture");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    fs::write(workspace.staging.join("abandoned.bin"), b"temporary").unwrap();
    advance_to_running(&state, &job.id);

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.interrupted, 1);
    assert!(!workspace.root.exists());
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Interrupted
    );
}

#[test]
fn publishing_job_completes_only_when_recorded_final_hash_and_size_match() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"verified crash fixture");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"verified crash fixture").unwrap();
    let (size, hash) = hash_file(&staging).unwrap();
    advance_to_running(&state, &job.id);
    advance(
        &state,
        &job.id,
        JobState::Running,
        JobState::Verifying,
        OperationStage::Verify,
    )
    .unwrap();
    state
        .database()
        .set_output_staging(
            &job.id,
            &staging.to_string_lossy(),
            size,
            &hash,
            "2026-08-16T12:00:00Z",
        )
        .unwrap();
    advance(
        &state,
        &job.id,
        JobState::Verifying,
        JobState::Publishing,
        OperationStage::Publish,
    )
    .unwrap();
    let final_path = destination.path().join("recovery-copy.bin");
    fs::write(&final_path, b"verified crash fixture").unwrap();
    state
        .database()
        .set_publication_intent(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.completed_publications, 1);
    assert!(!workspace.root.exists());
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Completed
    );
    assert_eq!(fs::read(final_path).unwrap(), b"verified crash fixture");
}

#[test]
fn publishing_mismatch_stays_interrupted_and_never_deletes_unknown_final_file() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"expected bytes");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"expected bytes").unwrap();
    let (size, hash) = hash_file(&staging).unwrap();
    advance_to_running(&state, &job.id);
    advance(
        &state,
        &job.id,
        JobState::Running,
        JobState::Verifying,
        OperationStage::Verify,
    )
    .unwrap();
    state
        .database()
        .set_output_staging(
            &job.id,
            &staging.to_string_lossy(),
            size,
            &hash,
            "2026-08-16T12:00:00Z",
        )
        .unwrap();
    advance(
        &state,
        &job.id,
        JobState::Verifying,
        JobState::Publishing,
        OperationStage::Publish,
    )
    .unwrap();
    let final_path = destination.path().join("recovery-copy.bin");
    fs::write(&final_path, b"competing or corrupted file").unwrap();
    state
        .database()
        .set_publication_intent(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.interrupted, 1);
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Interrupted
    );
    assert_eq!(
        fs::read(final_path).unwrap(),
        b"competing or corrupted file"
    );
}

#[test]
fn queued_job_remains_queued_for_rescheduling() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"queued");
    let job = create_job(&service, &input, destination.path());
    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.queued, 1);
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Queued
    );
}
