use std::path::Path;

use crate::app_state::AppState;
use crate::contracts::{JobState, OperationStage};
use crate::database::DatabaseError;
use crate::publication::hash_file;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub queued: usize,
    pub interrupted: usize,
    pub completed_publications: usize,
    pub cleanup_failures: usize,
}

pub fn reconcile_startup(state: &AppState) -> Result<RecoveryReport, DatabaseError> {
    let jobs = state.database().in_flight_jobs()?;
    let mut report = RecoveryReport::default();
    for job in jobs {
        if job.state == JobState::Queued {
            report.queued += 1;
            continue;
        }

        if job.state == JobState::Interrupted {
            if state.workspaces.cleanup_job(&job.id).is_err() {
                report.cleanup_failures += 1;
            } else {
                state.database().clear_ephemeral_paths(&job.id)?;
            }
            report.interrupted += 1;
            continue;
        }

        if job.state == JobState::Publishing && publication_proof_matches(state, &job.id)? {
            if state.workspaces.cleanup_job(&job.id).is_err() {
                state
                    .database()
                    .mark_interrupted(&job.id, JobState::Publishing)?;
                report.cleanup_failures += 1;
                report.interrupted += 1;
                continue;
            }
            let evidence = state
                .database()
                .publication_evidence(&job.id)?
                .ok_or(DatabaseError::TerminalEvidenceMissing)?;
            {
                let mut database = state.database();
                database.set_output_published(
                    &job.id,
                    &evidence.resolved_name,
                    &evidence.final_path,
                    evidence.size_bytes,
                    &evidence.sha256,
                )?;
                database.clear_ephemeral_paths(&job.id)?;
                let current = database
                    .get_job(&job.id)?
                    .ok_or(DatabaseError::JobConflict)?;
                database.transition_job(
                    &job.id,
                    JobState::Publishing,
                    current.version,
                    JobState::Completed,
                    Some(OperationStage::Recovery),
                )?;
            }
            report.completed_publications += 1;
            continue;
        }

        if state.workspaces.cleanup_job(&job.id).is_err() {
            report.cleanup_failures += 1;
        } else {
            state.database().clear_ephemeral_paths(&job.id)?;
        }
        state.database().mark_interrupted(&job.id, job.state)?;
        report.interrupted += 1;
    }
    Ok(report)
}

fn publication_proof_matches(state: &AppState, job_id: &str) -> Result<bool, DatabaseError> {
    let Some(evidence) = state.database().publication_evidence(job_id)? else {
        return Ok(false);
    };
    let final_path = Path::new(&evidence.final_path);
    if !final_path.is_file() {
        return Ok(false);
    }
    let Ok((size, hash)) = hash_file(final_path) else {
        return Ok(false);
    };
    Ok(size == evidence.size_bytes && hash == evidence.sha256)
}
