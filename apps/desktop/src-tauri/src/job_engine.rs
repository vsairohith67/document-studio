use crate::contracts::{JobRecord, JobState};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransitionError {
    #[error("job state changed: expected {expected:?}, found {actual:?}")]
    StateConflict {
        expected: JobState,
        actual: JobState,
    },
    #[error("job version changed: expected {expected}, found {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    #[error("illegal job transition from {from:?} to {to:?}")]
    IllegalTransition { from: JobState, to: JobState },
}

pub const fn can_transition(from: JobState, to: JobState) -> bool {
    use JobState::{
        Cancelled, Completed, Failed, Inspecting, Interrupted, Preflight, Publishing, Queued,
        Ready, Running, Verifying,
    };

    matches!(
        (from, to),
        (Queued, Inspecting | Cancelled | Failed)
            | (Inspecting, Preflight | Cancelled | Failed | Interrupted)
            | (Preflight, Ready | Cancelled | Failed | Interrupted)
            | (Ready, Running | Cancelled | Failed | Interrupted)
            | (Running, Verifying | Cancelled | Failed | Interrupted)
            | (Verifying, Publishing | Cancelled | Failed | Interrupted)
            | (Publishing, Completed | Failed | Interrupted)
            | (Interrupted, Preflight)
    )
}

pub const fn is_cancellable(state: JobState) -> bool {
    matches!(
        state,
        JobState::Queued
            | JobState::Inspecting
            | JobState::Preflight
            | JobState::Ready
            | JobState::Running
            | JobState::Verifying
    )
}

pub fn apply_transition(
    job: &mut JobRecord,
    expected_state: JobState,
    expected_version: u64,
    next: JobState,
    updated_at: String,
) -> Result<(), TransitionError> {
    if job.state != expected_state {
        return Err(TransitionError::StateConflict {
            expected: expected_state,
            actual: job.state,
        });
    }
    if job.version != expected_version {
        return Err(TransitionError::VersionConflict {
            expected: expected_version,
            actual: job.version,
        });
    }
    if !can_transition(job.state, next) {
        return Err(TransitionError::IllegalTransition {
            from: job.state,
            to: next,
        });
    }

    job.state = next;
    job.version += 1;
    job.updated_at = updated_at;
    if next.is_terminal() {
        job.finished_at = Some(job.updated_at.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{can_transition, is_cancellable};
    use crate::contracts::JobState;

    const STATES: [JobState; 11] = [
        JobState::Queued,
        JobState::Inspecting,
        JobState::Preflight,
        JobState::Ready,
        JobState::Running,
        JobState::Verifying,
        JobState::Publishing,
        JobState::Completed,
        JobState::Failed,
        JobState::Cancelled,
        JobState::Interrupted,
    ];

    #[test]
    fn terminal_states_have_no_outgoing_edges() {
        for from in [JobState::Completed, JobState::Failed, JobState::Cancelled] {
            assert!(STATES.iter().all(|to| !can_transition(from, *to)));
        }
    }

    #[test]
    fn lifecycle_cannot_skip_forward_or_move_backward() {
        assert!(!can_transition(JobState::Queued, JobState::Running));
        assert!(!can_transition(JobState::Running, JobState::Ready));
        assert!(!can_transition(JobState::Publishing, JobState::Cancelled));
        assert!(can_transition(JobState::Interrupted, JobState::Preflight));
    }

    #[test]
    fn cancellation_closes_before_publication_commit() {
        assert!(is_cancellable(JobState::Verifying));
        assert!(!is_cancellable(JobState::Publishing));
        assert!(!is_cancellable(JobState::Completed));
    }
}
