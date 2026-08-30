mod support;

use std::fs;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::{
    JobCompletionKind, JobRecord, JobState, JobsCreateRequest, OperationStage,
    BALANCED_COMPRESSION_OPERATION_ID, BALANCED_COMPRESSION_VERSION,
};
use document_studio_lib::database::{Database, DatabaseError};
use document_studio_lib::diagnostic_copy::DiagnosticCopyService;
use document_studio_lib::pdf_merge::PdfMergeService;
use document_studio_lib::publication::{hash_file, partial_ownership_result_code};
use document_studio_lib::recovery::{
    cancel_without_worker, reconcile_startup, resolve_interrupted,
};
use document_studio_lib::text_to_pdf::{TEXT_TO_PDF_OPERATION_ID, TEXT_TO_PDF_VERSION};
use document_studio_lib::windows_security::file_identity;
use document_studio_lib::workspace::WorkspaceManager;
use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

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

fn job_destination(job: &JobRecord) -> &Path {
    Path::new(&job.destination_directory)
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

fn activate_partial(state: &AppState, job_id: &str, partial_path: &Path) {
    let destination = partial_path.parent().unwrap();
    let result_code = partial_ownership_result_code(
        destination,
        job_id,
        partial_path,
        file_identity(partial_path).unwrap(),
    )
    .unwrap();
    state
        .database()
        .activate_owned_partial(job_id, &partial_path.to_string_lossy(), &result_code)
        .unwrap();
}

#[test]
fn startup_fails_running_job_after_proven_cleanup() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"crash fixture");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    fs::write(workspace.staging.join("abandoned.bin"), b"temporary").unwrap();
    advance_to_running(&state, &job.id);

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert!(!workspace.root.exists());
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
}

#[test]
fn startup_leaves_valid_completed_no_benefit_metadata_and_unknown_files_untouched() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"no-benefit source");
    let job = create_job(&service, &input, destination.path());
    state.workspaces.cleanup_job(&job.id).unwrap();
    let unknown = destination.path().join("recovery-copy.bin");
    fs::write(&unknown, b"unknown neighboring file").unwrap();
    {
        let database = state.database();
        database
            .connection()
            .execute("DELETE FROM job_outputs WHERE job_id = ?1", [&job.id])
            .unwrap();
        database
            .connection()
            .execute(
                "UPDATE jobs
                 SET state = 'completed', stage = NULL, completed_units = total_units,
                     resolved_output_name = NULL, finished_at = ?2, updated_at = ?2
                 WHERE id = ?1",
                (&job.id, "2026-08-25T12:00:00Z"),
            )
            .unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO job_completion_outcomes
                 (job_id, completion_kind, reason, created_at)
                 VALUES (?1, 'no-benefit', 'savings-threshold-not-met', ?2)",
                (&job.id, "2026-08-25T12:00:00Z"),
            )
            .unwrap();
    }

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report, Default::default());
    let completed = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        completed.completion_kind,
        Some(document_studio_lib::contracts::JobCompletionKind::NoBenefit)
    );
    assert!(completed.outputs.is_empty());
    assert_eq!(fs::read(&unknown).unwrap(), b"unknown neighboring file");
}

#[test]
fn startup_rejects_no_benefit_with_output_evidence_without_deleting_any_file() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"invalid outcome source");
    let job = create_job(&service, &input, destination.path());
    let unknown = destination.path().join("unknown.pdf");
    fs::write(&unknown, b"unknown neighboring file").unwrap();
    {
        let database = state.database();
        database
            .connection()
            .execute(
                "UPDATE jobs
                 SET state = 'completed', stage = NULL, completed_units = total_units,
                     resolved_output_name = NULL, finished_at = ?2, updated_at = ?2
                 WHERE id = ?1",
                (&job.id, "2026-08-25T12:00:00Z"),
            )
            .unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO job_completion_outcomes
                 (job_id, completion_kind, reason, created_at)
                 VALUES (?1, 'no-benefit', 'savings-threshold-not-met', ?2)",
                (&job.id, "2026-08-25T12:00:00Z"),
            )
            .unwrap();
    }

    assert!(matches!(
        reconcile_startup(&state),
        Err(DatabaseError::InvalidContractValue { .. })
    ));
    assert_eq!(fs::read(&unknown).unwrap(), b"unknown neighboring file");
}

#[test]
fn startup_resolves_inspecting_preflight_and_ready_without_resuming() {
    for target in [JobState::Inspecting, JobState::Preflight, JobState::Ready] {
        let (_app_data, state, service) = setup();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let input = support::write_fixture(source.path(), "input.bin", b"startup state");
        let job = create_job(&service, &input, destination.path());
        let workspace = state.workspaces.create_job(&job.id).unwrap();
        fs::write(workspace.staging.join("abandoned.bin"), b"temporary").unwrap();
        advance(
            &state,
            &job.id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )
        .unwrap();
        if target != JobState::Inspecting {
            advance(
                &state,
                &job.id,
                JobState::Inspecting,
                JobState::Preflight,
                OperationStage::Preflight,
            )
            .unwrap();
        }
        if target == JobState::Ready {
            advance(
                &state,
                &job.id,
                JobState::Preflight,
                JobState::Ready,
                OperationStage::Plan,
            )
            .unwrap();
        }

        let report = reconcile_startup(&state).unwrap();
        assert_eq!(report.failed, 1, "startup state {target:?}");
        assert!(!workspace.root.exists(), "startup state {target:?}");
        let recovered = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(
            recovered.state,
            JobState::Failed,
            "startup state {target:?}"
        );
        assert!(recovered
            .errors
            .iter()
            .any(|error| error.code == "JOB_INTERRUPTED_BY_RESTART"));
    }
}

#[test]
fn reserved_but_unactivated_partial_path_never_authorizes_deletion() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"reservation crash");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"reservation crash").unwrap();
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-77777777-7777-4777-8777-777777777777.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::write(
        &partial_path,
        b"unknown file created after reservation crash",
    )
    .unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(
        fs::read(&partial_path).unwrap(),
        b"unknown file created after reservation crash"
    );
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(recovered.outputs[0].partial_path.is_none());
}

#[test]
fn activated_identity_mismatch_preserves_preexisting_partial_after_release_failure() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"identity mismatch");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"identity mismatch").unwrap();
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-99999999-9999-4999-8999-999999999999.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    let guard_identity_source = job_destination(&job).join("owned-identity-source.tmp");
    fs::write(&guard_identity_source, b"").unwrap();
    let ownership_result_code = partial_ownership_result_code(
        job_destination(&job),
        &job.id,
        &partial_path,
        file_identity(&guard_identity_source).unwrap(),
    )
    .unwrap();
    state
        .database()
        .activate_owned_partial(
            &job.id,
            &partial_path.to_string_lossy(),
            &ownership_result_code,
        )
        .unwrap();
    fs::remove_file(guard_identity_source).unwrap();
    fs::write(&partial_path, b"pre-existing file at hard-link destination").unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(
        fs::read(&partial_path).unwrap(),
        b"pre-existing file at hard-link destination"
    );
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(recovered.outputs[0].partial_path.is_none());
}

#[test]
fn balanced_interrupted_publication_recovers_with_durable_published_outcome() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"verified crash fixture");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET operation_id = ?1, operation_version = ?2 WHERE id = ?3",
            (
                BALANCED_COMPRESSION_OPERATION_ID,
                BALANCED_COMPRESSION_VERSION,
                &job.id,
            ),
        )
        .unwrap();
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-44444444-4444-4444-8444-444444444444.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    state
        .database()
        .begin_publication(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::write(&final_path, b"verified crash fixture").unwrap();
    state
        .database()
        .set_output_published(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
            Some(&partial_path.to_string_lossy()),
        )
        .unwrap();
    let publishing = state.database().get_job(&job.id).unwrap().unwrap();
    state
        .database()
        .mark_interrupted(&job.id, publishing.state)
        .unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.completed_publications, 1);
    assert!(!workspace.root.exists());
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Completed);
    assert_eq!(
        recovered.completion_kind,
        Some(JobCompletionKind::Published)
    );
    assert_eq!(recovered.reason, None);
    assert_eq!(fs::read(final_path).unwrap(), b"verified crash fixture");
}

#[test]
fn txt_recovery_adopts_matching_final_after_post_move_metadata_failure() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.txt", b"committed txt pdf");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET operation_id = ?1, operation_version = ?2 WHERE id = ?3",
            (TEXT_TO_PDF_OPERATION_ID, TEXT_TO_PDF_VERSION, &job.id),
        )
        .unwrap();
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("normalized.pdf");
    fs::write(&staging, b"committed txt pdf").unwrap();
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
            "2026-08-30T07:00:00Z",
        )
        .unwrap();
    let final_path = destination.path().join("recovery-copy.bin");
    let partial_path = destination.path().join(format!(
        ".document-studio-{}-66666666-6666-4666-8666-666666666666.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    state
        .database()
        .begin_publication(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::rename(&staging, &final_path).unwrap();
    let generation = "99999999-9999-4999-8999-999999999999";
    let udf = workspace.temporary.join("renderer-udfs").join(generation);
    fs::create_dir_all(&udf).unwrap();
    fs::write(
        udf.join(".document-studio-txt-renderer-v1"),
        format!("job={}\ngeneration={generation}\n", job.id),
    )
    .unwrap();
    let locked_path = udf.join("owned.lock");
    fs::write(&locked_path, b"owned").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&locked_path)
        .unwrap();

    let first = reconcile_startup(&state).unwrap();
    assert_eq!(first.cleanup_failures, 1);
    assert_eq!(first.interrupted, 1);
    let interrupted = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    assert_eq!(interrupted.outputs[0].status.as_str(), "published");
    assert_eq!(fs::read(&final_path).unwrap(), b"committed txt pdf");

    drop(locked);
    let recovered = resolve_interrupted(&state, &job.id).unwrap();
    assert_eq!(recovered.state, JobState::Completed);
    assert_eq!(recovered.outputs[0].status.as_str(), "published");
    assert!(recovered.outputs[0].partial_path.is_none());
    assert_eq!(fs::read(final_path).unwrap(), b"committed txt pdf");
}

#[test]
fn txt_recovery_preserves_mismatched_commit_ambiguity_and_durable_intent() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.txt", b"expected txt pdf");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET operation_id = ?1, operation_version = ?2 WHERE id = ?3",
            (TEXT_TO_PDF_OPERATION_ID, TEXT_TO_PDF_VERSION, &job.id),
        )
        .unwrap();
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("normalized.pdf");
    fs::write(&staging, b"expected txt pdf").unwrap();
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
            "2026-08-30T07:00:00Z",
        )
        .unwrap();
    let final_path = destination.path().join("recovery-copy.bin");
    let partial_path = destination.path().join(format!(
        ".document-studio-{}-77777777-7777-4777-8777-777777777777.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    state
        .database()
        .begin_publication(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::write(&final_path, b"unknown competing bytes").unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.interrupted, 1);
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Interrupted);
    assert_eq!(recovered.outputs[0].status.as_str(), "publishing");
    assert_eq!(
        recovered.outputs[0].final_path.as_deref(),
        Some(final_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        recovered.outputs[0].partial_path.as_deref(),
        Some(partial_path.to_string_lossy().as_ref())
    );
    assert!(recovered.outputs[0].staging_path.is_none());
    assert!(recovered
        .errors
        .iter()
        .any(|error| { error.code == "TXT_PUBLICATION_RECONCILIATION_REQUIRED" }));
    assert_eq!(fs::read(final_path).unwrap(), b"unknown competing bytes");
}

#[test]
fn publishing_mismatch_fails_after_cleanup_and_never_deletes_unknown_final_file() {
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-55555555-5555-4555-8555-555555555555.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    state
        .database()
        .begin_publication(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::write(&final_path, b"competing or corrupted file").unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
    assert_eq!(
        fs::read(final_path).unwrap(),
        b"competing or corrupted file"
    );
}

#[test]
fn multi_output_recovery_preserves_published_files_and_fails_partial_publication_truthfully() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"multi output recovery");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "INSERT INTO job_outputs (
            job_id, ordinal, requested_name, mime_type, status
         ) VALUES (?1, 1, 'part-002.pdf', 'application/pdf', 'planned')",
            [&job.id],
        )
        .unwrap();
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let first_staging = workspace.staging.join("part-001.pdf");
    let second_staging = workspace.staging.join("part-002.pdf");
    fs::write(&first_staging, b"first verified output").unwrap();
    fs::write(&second_staging, b"second verified output").unwrap();
    let (first_size, first_hash) = hash_file(&first_staging).unwrap();
    let (second_size, second_hash) = hash_file(&second_staging).unwrap();
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
        .set_output_staging_at(
            &job.id,
            0,
            &first_staging.to_string_lossy(),
            first_size,
            &first_hash,
            "2026-08-17T12:00:00Z",
        )
        .unwrap();
    state
        .database()
        .set_output_staging_at(
            &job.id,
            1,
            &second_staging.to_string_lossy(),
            second_size,
            &second_hash,
            "2026-08-17T12:00:00Z",
        )
        .unwrap();
    let first_final = destination.path().join("part-001.pdf");
    let first_partial = destination.path().join(format!(
        ".document-studio-{}-33333333-3333-4333-8333-333333333333.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt_at(
            &job.id,
            0,
            "part-001.pdf",
            &first_final.to_string_lossy(),
            &first_partial.to_string_lossy(),
            first_size,
            &first_hash,
        )
        .unwrap();
    state
        .database()
        .begin_publication_at(
            &job.id,
            0,
            "part-001.pdf",
            &first_final.to_string_lossy(),
            first_size,
            &first_hash,
        )
        .unwrap();
    fs::write(&first_final, b"first verified output").unwrap();
    state
        .database()
        .set_output_published_at(
            &job.id,
            0,
            "part-001.pdf",
            &first_final.to_string_lossy(),
            first_size,
            &first_hash,
            Some(&first_partial.to_string_lossy()),
        )
        .unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(fs::read(&first_final).unwrap(), b"first verified output");
    assert!(!workspace.root.exists());
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert_eq!(recovered.outputs.len(), 2);
    assert_eq!(recovered.outputs[0].status.as_str(), "published");
    assert_ne!(recovered.outputs[1].status.as_str(), "published");
    assert!(recovered
        .outputs
        .iter()
        .all(|output| output.staging_path.is_none() && output.partial_path.is_none()));
    assert!(recovered
        .errors
        .iter()
        .any(|error| error.code == "PARTIAL_PUBLICATION"));
}

#[test]
fn queued_job_fails_deterministically_without_rescheduling() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"queued");
    let job = create_job(&service, &input, destination.path());
    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.queued, 1);
    assert_eq!(report.failed, 1);
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
}

#[test]
fn legacy_verifying_with_null_partial_is_quarantined_and_unknown_files_survive() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"legacy");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET operation_version = '1.0.0' WHERE id = ?1",
            [&job.id],
        )
        .unwrap();
    advance_to_running(&state, &job.id);
    advance(
        &state,
        &job.id,
        JobState::Running,
        JobState::Verifying,
        OperationStage::Verify,
    )
    .unwrap();
    let unknown = job_destination(&job).join(format!(
        ".document-studio-{}-11111111-1111-4111-8111-111111111111.partial",
        job.id
    ));
    fs::write(&unknown, b"unknown legacy file").unwrap();

    let report = reconcile_startup(&state).unwrap();
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(report.legacy_unproven, 1);
    assert_eq!(recovered.state, JobState::Interrupted);
    assert!(recovered
        .errors
        .iter()
        .any(|error| error.code == "LEGACY_CLEANUP_UNPROVEN"));
    assert!(unknown.exists());

    let resolution = resolve_interrupted(&state, &job.id).unwrap_err();
    assert_eq!(resolution.code, "LEGACY_CLEANUP_UNPROVEN");
    assert!(unknown.exists());
}

#[test]
fn legacy_publishing_and_interrupted_with_null_partial_remain_unresolved() {
    for legacy_state in [JobState::Publishing, JobState::Interrupted] {
        let (_app_data, state, service) = setup();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let input = support::write_fixture(source.path(), "input.bin", b"legacy publication");
        let job = create_job(&service, &input, destination.path());
        state
            .database()
            .connection()
            .execute(
                "UPDATE jobs SET operation_version = '1.0.0', state = ?1, stage = 'recovery'
                 WHERE id = ?2",
                [legacy_state.as_str(), &job.id],
            )
            .unwrap();
        let unknown = job_destination(&job).join(format!(
            ".document-studio-{}-88888888-8888-4888-8888-888888888888.partial",
            job.id
        ));
        fs::write(&unknown, b"unknown legacy partial").unwrap();

        let report = reconcile_startup(&state).unwrap();
        assert_eq!(report.legacy_unproven, 1, "legacy state {legacy_state:?}");
        let recovered = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(recovered.state, JobState::Interrupted);
        assert!(recovered.outputs[0].partial_path.is_none());
        assert!(recovered
            .errors
            .iter()
            .any(|error| error.code == "LEGACY_CLEANUP_UNPROVEN"));
        assert_eq!(fs::read(&unknown).unwrap(), b"unknown legacy partial");
        let resolution = resolve_interrupted(&state, &job.id).unwrap_err();
        assert_eq!(resolution.code, "LEGACY_CLEANUP_UNPROVEN");
    }
}

#[test]
fn legacy_prepublication_job_can_be_proven_clean_and_failed() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"legacy prepublication");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET operation_version = '1.0.0' WHERE id = ?1",
            [&job.id],
        )
        .unwrap();
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    fs::write(workspace.staging.join("abandoned.bin"), b"temporary").unwrap();
    advance_to_running(&state, &job.id);

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert!(!workspace.root.exists());
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    let proven: i64 = state
        .database()
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM job_stage_runs WHERE job_id = ?1 AND safe_result_code = 'LEGACY_CLEANUP_PROVEN'",
            [&job.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(proven, 1);
    reconcile_startup(&state).unwrap();
    let reopened = state.database().get_job(&job.id).unwrap().unwrap();
    assert!(!reopened
        .errors
        .iter()
        .any(|error| error.code == "LEGACY_CLEANUP_UNPROVEN"));
}

#[test]
fn legacy_terminal_states_keep_their_outcome_and_receive_one_warning() {
    for (index, terminal) in [JobState::Completed, JobState::Failed, JobState::Cancelled]
        .into_iter()
        .enumerate()
    {
        let (_app_data, state, service) = setup();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let input = support::write_fixture(source.path(), "input.bin", b"legacy terminal");
        let job = create_job(&service, &input, destination.path());
        state
            .database()
            .connection()
            .execute(
                "UPDATE jobs SET operation_version = '1.0.0', state = ?1,
                    finished_at = '2026-08-01T00:00:00Z' WHERE id = ?2",
                [terminal.as_str(), &job.id],
            )
            .unwrap();
        let neighbor = job_destination(&job).join(format!("neighbor-{index}.bin"));
        fs::write(&neighbor, b"preserve").unwrap();

        reconcile_startup(&state).unwrap();
        reconcile_startup(&state).unwrap();
        let recovered = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(recovered.state, terminal);
        assert_eq!(
            recovered
                .errors
                .iter()
                .filter(|error| error.code == "LEGACY_CLEANUP_UNPROVEN")
                .count(),
            1
        );
        assert_eq!(fs::read(neighbor).unwrap(), b"preserve");
    }
}

#[test]
fn restart_removes_the_exact_identity_activated_partial_before_any_bytes() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"restart journal");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"restart journal").unwrap();
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-22222222-2222-4222-8222-222222222222.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .unwrap();
    activate_partial(&state, &job.id, &partial_path);
    let neighbor = job_destination(&job).join("neighbor.bin");
    fs::write(&neighbor, b"preserve").unwrap();

    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert!(!partial_path.exists());
    assert_eq!(fs::read(neighbor).unwrap(), b"preserve");
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(recovered.outputs[0].partial_path.is_none());
}

#[test]
fn restart_keeps_exact_ownership_when_partial_deletion_fails_then_retries_safely() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"restart copy");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();
    let staging = workspace.staging.join("output.staging");
    fs::write(&staging, b"restart copy").unwrap();
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
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-33333333-3333-4333-8333-333333333333.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            size,
            &hash,
        )
        .unwrap();
    let mut partial = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .unwrap();
    partial.write_all(b"incomplete").unwrap();
    drop(partial);
    activate_partial(&state, &job.id, &partial_path);
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&partial_path)
        .unwrap();
    let first = reconcile_startup(&state).unwrap();
    assert_eq!(first.cleanup_failures, 1);
    let interrupted = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    assert_eq!(
        interrupted.outputs[0].partial_path.as_deref(),
        Some(partial_path.to_string_lossy().as_ref())
    );
    assert!(partial_path.exists());

    drop(lock);
    let second = reconcile_startup(&state).unwrap();
    assert_eq!(second.failed, 1);
    let recovered = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(recovered.outputs[0].partial_path.is_none());
    assert!(!partial_path.exists());
}

#[test]
fn no_token_cancellation_fallback_requires_exact_cleanup_evidence() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"fallback");
    let job = create_job(&service, &input, destination.path());
    let workspace = state.workspaces.create_job(&job.id).unwrap();

    let cancelled = cancel_without_worker(&state, &job.id).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(!workspace.root.exists());
}

#[test]
fn no_token_cancellation_never_terminalizes_when_partial_deletion_fails() {
    let (_app_data, state, service) = setup();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source.path(), "input.bin", b"fallback failure");
    let job = create_job(&service, &input, destination.path());
    state
        .database()
        .connection()
        .execute(
            "UPDATE jobs SET state = 'verifying' WHERE id = ?1",
            [&job.id],
        )
        .unwrap();
    state
        .database()
        .connection()
        .execute(
            "UPDATE job_outputs SET status = 'verified' WHERE job_id = ?1",
            [&job.id],
        )
        .unwrap();
    let final_path = job_destination(&job).join("recovery-copy.bin");
    let partial_path = job_destination(&job).join(format!(
        ".document-studio-{}-66666666-6666-4666-8666-666666666666.partial",
        job.id
    ));
    state
        .database()
        .reserve_publication_attempt(
            &job.id,
            "recovery-copy.bin",
            &final_path.to_string_lossy(),
            &partial_path.to_string_lossy(),
            1,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
    fs::write(&partial_path, b"owned").unwrap();
    activate_partial(&state, &job.id, &partial_path);
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&partial_path)
        .unwrap();

    let error = cancel_without_worker(&state, &job.id).unwrap_err();
    assert_eq!(error.code, "CLEANUP_FAILED");
    let interrupted = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    assert_eq!(
        interrupted.outputs[0].partial_path.as_deref(),
        Some(partial_path.to_string_lossy().as_ref())
    );
    drop(lock);
}

#[test]
fn pdf_merge_restarts_fail_every_prepublication_state_without_resuming() {
    for target in [
        JobState::Queued,
        JobState::Inspecting,
        JobState::Preflight,
        JobState::Ready,
        JobState::Running,
        JobState::Verifying,
    ] {
        let (_app_data, state, _diagnostic) = setup();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let first = support::write_pdf_fixture(source.path(), "first.pdf", "FIRST", 600);
        let second = support::write_pdf_fixture(source.path(), "second.pdf", "SECOND", 601);
        let job = PdfMergeService::new(state.clone())
            .create_job(JobsCreateRequest {
                operation_id: "pdf.merge".to_owned(),
                input_paths: vec![
                    first.to_string_lossy().into_owned(),
                    second.to_string_lossy().into_owned(),
                ],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "recovered.pdf".to_owned(),
            })
            .unwrap();
        let workspace = state.workspaces.create_job(&job.id).unwrap();
        fs::write(workspace.inputs.join("source-0000.pdf"), b"owned temporary").unwrap();
        if target != JobState::Queued {
            advance(
                &state,
                &job.id,
                JobState::Queued,
                JobState::Inspecting,
                OperationStage::Inspect,
            )
            .unwrap();
        }
        if matches!(
            target,
            JobState::Preflight | JobState::Ready | JobState::Running | JobState::Verifying
        ) {
            advance(
                &state,
                &job.id,
                JobState::Inspecting,
                JobState::Preflight,
                OperationStage::Preflight,
            )
            .unwrap();
        }
        if matches!(
            target,
            JobState::Ready | JobState::Running | JobState::Verifying
        ) {
            advance(
                &state,
                &job.id,
                JobState::Preflight,
                JobState::Ready,
                OperationStage::Plan,
            )
            .unwrap();
        }
        if matches!(target, JobState::Running | JobState::Verifying) {
            advance(
                &state,
                &job.id,
                JobState::Ready,
                JobState::Running,
                OperationStage::Execute,
            )
            .unwrap();
        }
        if target == JobState::Verifying {
            advance(
                &state,
                &job.id,
                JobState::Running,
                JobState::Verifying,
                OperationStage::Verify,
            )
            .unwrap();
        }

        let report = reconcile_startup(&state).unwrap();
        assert_eq!(report.failed, 1, "PDF Merge restart state {target:?}");
        let recovered = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(
            recovered.state,
            JobState::Failed,
            "PDF Merge state {target:?}"
        );
        assert!(!workspace.root.exists());
        let restart_error = recovered
            .errors
            .iter()
            .find(|error| {
                error.code
                    == if target == JobState::Queued {
                        "JOB_WORKER_NOT_STARTED"
                    } else {
                        "JOB_INTERRUPTED_BY_RESTART"
                    }
            })
            .expect("sanitized restart error");
        assert!(restart_error
            .detail
            .contains("Document Studio does not automatically resume interrupted jobs"));
        assert!(!restart_error.detail.contains("G01"));
        assert!(!restart_error.detail.contains("will automatically resume"));
        assert!(!restart_error.detail.contains("automatically resumes"));
        assert!(restart_error.detail.len() <= 160);
        assert!(!restart_error.detail.contains("first.pdf"));
        assert!(!restart_error.detail.contains("second.pdf"));
        assert!(!restart_error
            .detail
            .contains(&destination.path().to_string_lossy().to_string()));
    }
}
