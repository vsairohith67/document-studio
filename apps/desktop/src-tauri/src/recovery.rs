use std::path::Path;

use crate::app_state::AppState;
use crate::contracts::{
    JobRecord, JobState, OperationError, OperationStage, BALANCED_COMPRESSION_OPERATION_ID,
    DIAGNOSTIC_COPY_OPERATION_ID, LEGACY_CLEANUP_PROVEN, LEGACY_CLEANUP_UNPROVEN,
    LEGACY_DIAGNOSTIC_COPY_VERSION,
};
use crate::database::{Database, DatabaseError};
use crate::publication::{hash_file, is_exact_owned_partial_path, partial_ownership_result_code};
use crate::windows_security::{delete_open_file, identity_from_file, open_for_identity_and_delete};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub queued: usize,
    pub failed: usize,
    pub interrupted: usize,
    pub completed_publications: usize,
    pub cleanup_failures: usize,
    pub legacy_unproven: usize,
}

pub fn reconcile_startup(state: &AppState) -> Result<RecoveryReport, DatabaseError> {
    let jobs = state.database().startup_recovery_jobs()?;
    let mut report = RecoveryReport::default();
    for job in jobs {
        reconcile_job(state, job, &mut report)?;
    }
    Ok(report)
}

pub fn resolve_interrupted(state: &AppState, job_id: &str) -> Result<JobRecord, OperationError> {
    let job = state
        .database()
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)?;
    if job.state != JobState::Interrupted {
        return Err(recovery_not_found());
    }
    if is_legacy(&job)
        && !state
            .database()
            .legacy_cleanup_is_proven(job_id)
            .map_err(|_| recovery_metadata_error())?
    {
        state
            .database()
            .record_error_once(job_id, &legacy_unproven_error())
            .map_err(|_| recovery_metadata_error())?;
        return Err(legacy_unproven_error());
    }

    let mut report = RecoveryReport::default();
    reconcile_current_job(state, job, &mut report).map_err(|_| recovery_metadata_error())?;
    state
        .database()
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)
}

pub fn resolve_worker_spawn_failure(state: &AppState, job_id: &str) -> Result<(), OperationError> {
    let job = state
        .database()
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)?;
    if job.state != JobState::Queued {
        return Err(recovery_not_found());
    }
    let mut report = RecoveryReport::default();
    reconcile_current_job(state, job, &mut report).map_err(|_| recovery_metadata_error())
}

pub fn cancel_without_worker(state: &AppState, job_id: &str) -> Result<JobRecord, OperationError> {
    let job = state
        .database()
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)?;
    if !crate::job_engine::is_cancellable(job.state) {
        return Err(recovery_not_found());
    }
    if is_legacy(&job)
        && !state
            .database()
            .legacy_cleanup_is_proven(job_id)
            .map_err(|_| recovery_metadata_error())?
    {
        state
            .database()
            .record_error_once(job_id, &legacy_unproven_error())
            .map_err(|_| recovery_metadata_error())?;
        return Err(legacy_unproven_error());
    }
    if !cleanup_partial(state, &job).map_err(|_| recovery_metadata_error())?
        || !cleanup_workspace(state, &job).map_err(|_| recovery_metadata_error())?
    {
        interrupt_with_cleanup_error(state, &job).map_err(|_| recovery_metadata_error())?;
        return Err(cleanup_error());
    }
    let mut database = state.database();
    for output in job
        .outputs
        .iter()
        .filter(|output| output.status != crate::contracts::OutputStatus::Published)
    {
        database
            .clear_unpublished_intent_at(job_id, output.ordinal)
            .map_err(|_| recovery_metadata_error())?;
    }
    let current = database
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)?;
    database
        .transition_job(
            job_id,
            current.state,
            current.version,
            JobState::Cancelled,
            Some(OperationStage::Cleanup),
        )
        .map_err(|_| recovery_metadata_error())?;
    database
        .get_job(job_id)
        .map_err(|_| recovery_metadata_error())?
        .ok_or_else(recovery_not_found)
}

fn reconcile_job(
    state: &AppState,
    job: JobRecord,
    report: &mut RecoveryReport,
) -> Result<(), DatabaseError> {
    if is_legacy(&job) {
        return reconcile_legacy_job(state, job, report);
    }
    reconcile_current_job(state, job, report)
}

fn reconcile_legacy_job(
    state: &AppState,
    job: JobRecord,
    report: &mut RecoveryReport,
) -> Result<(), DatabaseError> {
    if state.database().legacy_cleanup_is_proven(&job.id)? {
        if job.state.is_terminal() {
            return Ok(());
        }
        return reconcile_current_job(state, job, report);
    }
    let before_destination_publication = matches!(
        job.state,
        JobState::Queued
            | JobState::Inspecting
            | JobState::Preflight
            | JobState::Ready
            | JobState::Running
    );
    let has_recorded_partial = job
        .outputs
        .iter()
        .any(|output| output.partial_path.is_some());

    if before_destination_publication && !has_recorded_partial {
        if cleanup_workspace(state, &job)? {
            let mut database = state.database();
            database.record_recovery_result_once(&job.id, LEGACY_CLEANUP_PROVEN)?;
            database.record_error_once(&job.id, &worker_stopped_error(job.state))?;
            transition_to_failed(&mut database, &job)?;
            report.failed += 1;
        } else {
            interrupt_with_cleanup_error(state, &job)?;
            report.cleanup_failures += 1;
            report.interrupted += 1;
        }
        return Ok(());
    }

    let workspace_cleaned = cleanup_workspace(state, &job)?;
    let mut database = state.database();
    database.record_error_once(&job.id, &legacy_unproven_error())?;
    if !workspace_cleaned {
        database.record_error_once(&job.id, &cleanup_error())?;
        report.cleanup_failures += 1;
    }
    if !job.state.is_terminal() && job.state != JobState::Interrupted {
        database.mark_interrupted(&job.id, job.state)?;
    }
    report.legacy_unproven += 1;
    if !job.state.is_terminal() {
        report.interrupted += 1;
    }
    Ok(())
}

fn reconcile_current_job(
    state: &AppState,
    job: JobRecord,
    report: &mut RecoveryReport,
) -> Result<(), DatabaseError> {
    if job.state.is_terminal() {
        if has_temporary_metadata(&job)
            && (!cleanup_partial(state, &job)? || !cleanup_workspace(state, &job)?)
        {
            report.cleanup_failures += 1;
            return Err(DatabaseError::TerminalEvidenceMissing);
        }
        return Ok(());
    }

    if job.state == JobState::Queued {
        report.queued += 1;
    }

    if matches!(job.state, JobState::Publishing | JobState::Interrupted)
        && publication_proof_matches_all(state, &job)?
    {
        if !cleanup_partial(state, &job)? || !cleanup_workspace(state, &job)? {
            interrupt_with_cleanup_error(state, &job)?;
            report.cleanup_failures += 1;
            report.interrupted += 1;
            return Ok(());
        }
        let mut database = state.database();
        for output in &job.outputs {
            let evidence = database
                .publication_evidence_at(&job.id, output.ordinal)?
                .ok_or(DatabaseError::TerminalEvidenceMissing)?;
            database.set_output_published_at(
                &job.id,
                output.ordinal,
                &evidence.resolved_name,
                &evidence.final_path,
                evidence.size_bytes,
                &evidence.sha256,
                None,
            )?;
        }
        let current = database
            .get_job(&job.id)?
            .ok_or(DatabaseError::JobConflict)?;
        if job.operation_id == BALANCED_COMPRESSION_OPERATION_ID {
            database.complete_recovered_published(&job.id, current.state, current.version)?;
        } else {
            database.transition_job(
                &job.id,
                current.state,
                current.version,
                JobState::Completed,
                Some(OperationStage::Recovery),
            )?;
        }
        report.completed_publications += 1;
        return Ok(());
    }

    if cleanup_partial(state, &job)? && cleanup_workspace(state, &job)? {
        let mut database = state.database();
        let published = job
            .outputs
            .iter()
            .filter(|output| output.status == crate::contracts::OutputStatus::Published)
            .count();
        for output in job
            .outputs
            .iter()
            .filter(|output| output.status != crate::contracts::OutputStatus::Published)
        {
            database.clear_unpublished_intent_at(&job.id, output.ordinal)?;
        }
        if published > 0 {
            database.record_error_once(
                &job.id,
                &partial_publication_error(published, job.outputs.len()),
            )?;
        }
        database.record_error_once(&job.id, &worker_stopped_error(job.state))?;
        transition_to_failed(&mut database, &job)?;
        report.failed += 1;
    } else {
        interrupt_with_cleanup_error(state, &job)?;
        report.cleanup_failures += 1;
        report.interrupted += 1;
    }
    Ok(())
}

fn transition_to_failed(database: &mut Database, job: &JobRecord) -> Result<(), DatabaseError> {
    let current = database
        .get_job(&job.id)?
        .ok_or(DatabaseError::JobConflict)?;
    database.transition_job(
        &job.id,
        current.state,
        current.version,
        JobState::Failed,
        Some(OperationStage::Recovery),
    )?;
    Ok(())
}

fn interrupt_with_cleanup_error(state: &AppState, job: &JobRecord) -> Result<(), DatabaseError> {
    let mut database = state.database();
    database.record_error_once(&job.id, &cleanup_error())?;
    let current = database
        .get_job(&job.id)?
        .ok_or(DatabaseError::JobConflict)?;
    database.mark_interrupted(&job.id, current.state)
}

fn cleanup_partial(state: &AppState, job: &JobRecord) -> Result<bool, DatabaseError> {
    let destination = Path::new(&job.destination_directory);
    for output in &job.outputs {
        let Some(partial_path) = output.partial_path.as_deref() else {
            continue;
        };
        let partial = Path::new(partial_path);
        if !is_exact_owned_partial_path(destination, &job.id, partial) {
            return Ok(false);
        }
        match open_for_identity_and_delete(partial) {
            Ok(file) => {
                let identity = match identity_from_file(&file) {
                    Ok(identity) => identity,
                    Err(_) => return Ok(false),
                };
                let ownership_result_code =
                    partial_ownership_result_code(destination, &job.id, partial, identity)
                        .ok_or(DatabaseError::TerminalEvidenceMissing)?;
                let activated = state.database().owned_partial_is_activated_at(
                    &job.id,
                    output.ordinal,
                    partial_path,
                    &ownership_result_code,
                )?;
                if activated && delete_open_file(file).is_err() {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Ok(false),
        }
        state
            .database()
            .clear_owned_partial_at(&job.id, output.ordinal, partial_path)?;
    }
    Ok(true)
}

fn cleanup_workspace(state: &AppState, job: &JobRecord) -> Result<bool, DatabaseError> {
    if state.workspaces.cleanup_job(&job.id).is_err() {
        return Ok(false);
    }
    let mut database = state.database();
    for output in &job.outputs {
        database.clear_staging_path_at(&job.id, output.ordinal, output.staging_path.as_deref())?;
    }
    Ok(true)
}

fn has_temporary_metadata(job: &JobRecord) -> bool {
    job.outputs
        .iter()
        .any(|output| output.staging_path.is_some() || output.partial_path.is_some())
}

fn is_legacy(job: &JobRecord) -> bool {
    job.operation_id == DIAGNOSTIC_COPY_OPERATION_ID
        && job.operation_version == LEGACY_DIAGNOSTIC_COPY_VERSION
}

fn publication_proof_matches_all(state: &AppState, job: &JobRecord) -> Result<bool, DatabaseError> {
    if job.outputs.is_empty() {
        return Ok(false);
    }
    for output in &job.outputs {
        let Some(evidence) = state
            .database()
            .publication_evidence_at(&job.id, output.ordinal)?
        else {
            return Ok(false);
        };
        if evidence.status != crate::contracts::OutputStatus::Published {
            return Ok(false);
        }
        let final_path = Path::new(&evidence.final_path);
        if !final_path.is_file() {
            return Ok(false);
        }
        let Ok((size, hash)) = hash_file(final_path) else {
            return Ok(false);
        };
        if size != evidence.size_bytes || hash != evidence.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn partial_publication_error(published: usize, expected: usize) -> OperationError {
    OperationError::safe(
        "PARTIAL_PUBLICATION",
        "Only part of the split was published",
        format!(
            "{published} of {expected} verified outputs were published before processing stopped; published user files were preserved."
        ),
        OperationStage::Recovery,
        false,
    )
}

fn worker_stopped_error(state: JobState) -> OperationError {
    let code = if state == JobState::Queued {
        "JOB_WORKER_NOT_STARTED"
    } else {
        "JOB_INTERRUPTED_BY_RESTART"
    };
    OperationError::safe(
        code,
        "The job did not finish",
        "Temporary data was safely reconciled; Document Studio does not automatically resume interrupted jobs.",
        OperationStage::Recovery,
        true,
    )
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "CLEANUP_FAILED",
        "Temporary data could not be completely removed",
        "The job remains interrupted so exact cleanup can be retried safely.",
        OperationStage::Recovery,
        true,
    )
}

fn legacy_unproven_error() -> OperationError {
    OperationError::safe(
        LEGACY_CLEANUP_UNPROVEN,
        "Legacy destination cleanup is unproven",
        "Inspect the affected destination manually; unknown files are preserved.",
        OperationStage::Recovery,
        false,
    )
}

fn recovery_metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "Recovery metadata could not be saved",
        "The job cannot be resolved until its local metadata is available.",
        OperationStage::Recovery,
        true,
    )
}

fn recovery_not_found() -> OperationError {
    OperationError::safe(
        "JOB_NOT_RECOVERABLE",
        "The job cannot be resolved",
        "Refresh history and choose an interrupted non-legacy job.",
        OperationStage::Recovery,
        false,
    )
}
