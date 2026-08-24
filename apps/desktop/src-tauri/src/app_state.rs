use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::database::Database;
use crate::workspace::WorkspaceManager;

#[derive(Debug, Clone)]
pub struct CancellationToken {
    state: Arc<AtomicU8>,
}

const CANCELLATION_ACTIVE: u8 = 0;
const CANCELLATION_REQUESTED: u8 = 1;
const PUBLICATION_COMMIT_STARTED: u8 = 2;

impl CancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLATION_REQUESTED
    }

    pub fn try_begin_publication_commit(&self) -> bool {
        match self.state.compare_exchange(
            CANCELLATION_ACTIVE,
            PUBLICATION_COMMIT_STARTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(PUBLICATION_COMMIT_STARTED) => true,
            Err(CANCELLATION_REQUESTED) => false,
            Err(_) => false,
        }
    }

    pub fn commit_started(&self) -> bool {
        self.state.load(Ordering::Acquire) == PUBLICATION_COMMIT_STARTED
    }

    fn request_cancellation(&self) -> CancelOutcome {
        match self.state.compare_exchange(
            CANCELLATION_ACTIVE,
            CANCELLATION_REQUESTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(CANCELLATION_REQUESTED) => CancelOutcome::Requested,
            Err(PUBLICATION_COMMIT_STARTED) => CancelOutcome::TooLate,
            Err(_) => CancelOutcome::NotRunning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelOutcome, CancellationRegistry};
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn publication_commit_boundary_is_atomic_with_cancellation() {
        let registry = CancellationRegistry::default();
        let cancelled_first = registry.register("cancelled-first");
        assert_eq!(
            registry.request("cancelled-first"),
            CancelOutcome::Requested
        );
        assert!(!cancelled_first.try_begin_publication_commit());

        let commit_first = registry.register("commit-first");
        assert!(commit_first.try_begin_publication_commit());
        assert_eq!(registry.request("commit-first"), CancelOutcome::TooLate);
    }

    #[test]
    fn concurrent_cancel_and_commit_have_one_truthful_winner() {
        for attempt in 0..256 {
            let registry = Arc::new(CancellationRegistry::default());
            let job_id = format!("race-{attempt}");
            let token = registry.register(&job_id);
            let barrier = Arc::new(Barrier::new(3));

            let commit_barrier = barrier.clone();
            let commit = thread::spawn(move || {
                commit_barrier.wait();
                token.try_begin_publication_commit()
            });
            let cancel_barrier = barrier.clone();
            let cancel_registry = registry.clone();
            let cancel_job_id = job_id.clone();
            let cancel = thread::spawn(move || {
                cancel_barrier.wait();
                cancel_registry.request(&cancel_job_id)
            });

            barrier.wait();
            let commit_won = commit.join().unwrap();
            let cancel_outcome = cancel.join().unwrap();
            assert!(matches!(
                (commit_won, cancel_outcome),
                (true, CancelOutcome::TooLate) | (false, CancelOutcome::Requested)
            ));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelOutcome {
    Requested,
    TooLate,
    NotRunning,
}

#[derive(Debug, Default)]
pub struct CancellationRegistry {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl CancellationRegistry {
    pub fn register(&self, job_id: &str) -> CancellationToken {
        let token = CancellationToken {
            state: Arc::new(AtomicU8::new(CANCELLATION_ACTIVE)),
        };
        self.tokens
            .lock()
            .expect("cancellation registry mutex poisoned")
            .insert(job_id.to_owned(), token.clone());
        token
    }

    pub fn request(&self, job_id: &str) -> CancelOutcome {
        let tokens = self
            .tokens
            .lock()
            .expect("cancellation registry mutex poisoned");
        let Some(token) = tokens.get(job_id) else {
            return CancelOutcome::NotRunning;
        };
        token.request_cancellation()
    }

    pub fn unregister(&self, job_id: &str) {
        self.tokens
            .lock()
            .expect("cancellation registry mutex poisoned")
            .remove(job_id);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub database: Arc<Mutex<Database>>,
    pub workspaces: WorkspaceManager,
    pub cancellations: Arc<CancellationRegistry>,
    pub qpdf: Option<crate::qpdf::QpdfRuntimeManager>,
    pub viewer_sessions: crate::viewer_sessions::ViewerSessionManager,
    pub pdf_to_images_jobs: crate::pdf_to_images::PdfToImagesManager,
}

impl AppState {
    pub fn new(database: Database, workspaces: WorkspaceManager) -> Self {
        Self {
            database: Arc::new(Mutex::new(database)),
            workspaces,
            cancellations: Arc::new(CancellationRegistry::default()),
            qpdf: None,
            viewer_sessions: crate::viewer_sessions::ViewerSessionManager::default(),
            pdf_to_images_jobs: crate::pdf_to_images::PdfToImagesManager::default(),
        }
    }

    pub fn with_qpdf(mut self, qpdf: crate::qpdf::QpdfRuntimeManager) -> Self {
        self.qpdf = Some(qpdf);
        self
    }

    pub fn database(&self) -> MutexGuard<'_, Database> {
        self.database
            .lock()
            .expect("metadata database mutex poisoned")
    }
}
