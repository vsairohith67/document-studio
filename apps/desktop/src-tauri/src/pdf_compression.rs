use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{JobRecord, JobsCreateRequest, OperationError, ProgressEvent};
use crate::pdf_merge::{PdfMergeHooks, PdfMergeService};

#[derive(Clone)]
pub struct PdfCompressionService {
    lifecycle: PdfMergeService,
}

impl PdfCompressionService {
    pub fn new(state: AppState) -> Self {
        Self {
            lifecycle: PdfMergeService::new(state),
        }
    }

    pub fn create_job(&self, request: JobsCreateRequest) -> Result<JobRecord, OperationError> {
        self.lifecycle.create_lossless_compression_job(request)
    }

    pub fn execute_with_registered_token<F>(
        &self,
        job_id: &str,
        token: CancellationToken,
        on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.lifecycle
            .execute_with_registered_token(job_id, token, on_event)
    }

    pub fn execute<F>(&self, job_id: &str, on_event: F) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.lifecycle.execute(job_id, on_event)
    }

    #[doc(hidden)]
    pub fn execute_with_hooks<F>(
        &self,
        job_id: &str,
        on_event: F,
        hooks: PdfMergeHooks,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.lifecycle.execute_with_hooks(job_id, on_event, hooks)
    }
}
