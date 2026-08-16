use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ};

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    JobInput, JobOutput, JobProgress, JobRecord, JobState, JobsCreateRequest, OperationError,
    OperationStage, OutputStatus, ProgressEvent, ProgressUnit,
};
use crate::database::DatabaseError;
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
};
use crate::publication::{
    hash_file, publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::windows_security::{available_bytes, identity_from_file};
use crate::workspace::JobWorkspace;

const COPY_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct DiagnosticCopyHooks {
    pub fail_write_after_bytes: Option<u64>,
    pub corrupt_staging_before_verify: bool,
    pub create_collision_before_first_publication_commit: bool,
    pub fail_cleanup: bool,
}

#[derive(Clone)]
pub struct DiagnosticCopyService {
    state: AppState,
}

impl DiagnosticCopyService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn create_job(&self, request: JobsCreateRequest) -> Result<JobRecord, OperationError> {
        if request.operation_id != "diagnostic.copy" || request.input_paths.len() != 1 {
            return Err(safe_error(
                "INVALID_OPERATION_REQUEST",
                "The diagnostic copy request is not valid",
                "Choose exactly one local file for the diagnostic copy operation.",
                OperationStage::Inspect,
                false,
            ));
        }
        validate_output_name(&request.requested_output_name).map_err(|_| path_error())?;
        let input_path = Path::new(&request.input_paths[0]);
        let (canonical_input, identity) =
            canonical_regular_file(input_path).map_err(|_| path_error())?;
        let destination = canonical_directory(Path::new(&request.destination_directory))
            .map_err(|_| path_error())?;
        let metadata = fs::metadata(&canonical_input).map_err(|_| inspect_error())?;
        let modified: DateTime<Utc> = metadata.modified().map_err(|_| inspect_error())?.into();
        let now = timestamp();
        let id = Uuid::new_v4().hyphenated().to_string();
        let display_name = canonical_input
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(path_error)?
            .to_owned();
        let source_path = input_path.to_string_lossy().into_owned();
        let canonical_path = canonical_input.to_string_lossy().into_owned();
        let destination_directory = destination.to_string_lossy().into_owned();
        let job = JobRecord {
            id,
            operation_id: "diagnostic.copy".to_owned(),
            operation_version: "1.0.0".to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: metadata.len(),
                unit: ProgressUnit::Bytes,
            },
            destination_directory,
            requested_output_name: request.requested_output_name.clone(),
            resolved_output_name: None,
            cancellation_requested_at: None,
            created_at: now.clone(),
            updated_at: now,
            finished_at: None,
            version: 0,
            inputs: vec![JobInput {
                ordinal: 0,
                display_name,
                source_path,
                canonical_path,
                file_identity: identity.to_string(),
                size_bytes: metadata.len(),
                modified_at: modified.to_rfc3339_opts(SecondsFormat::Secs, true),
                mime_type: "application/octet-stream".to_owned(),
                sha256: None,
                password_reference: None,
            }],
            outputs: vec![JobOutput {
                ordinal: 0,
                requested_name: request.requested_output_name,
                resolved_name: None,
                staging_path: None,
                partial_path: None,
                final_path: None,
                size_bytes: None,
                mime_type: "application/octet-stream".to_owned(),
                sha256: None,
                status: OutputStatus::Planned,
                verified_at: None,
                published_at: None,
            }],
            errors: Vec::new(),
        };
        self.state
            .database()
            .create_job(&job)
            .map_err(|_| metadata_error())?;
        Ok(job)
    }

    pub fn execute<F>(&self, job_id: &str, on_event: F) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.execute_with_hooks(job_id, on_event, DiagnosticCopyHooks::default())
    }

    #[doc(hidden)]
    pub fn execute_with_hooks<F>(
        &self,
        job_id: &str,
        mut on_event: F,
        hooks: DiagnosticCopyHooks,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let token = self.state.cancellations.register(job_id);
        let mut workspace: Option<JobWorkspace> = None;
        let result = self.execute_inner(job_id, &token, &hooks, &mut workspace, &mut on_event);
        let result = match result {
            Ok(job) => Ok(job),
            Err(error) if error.code == "CANCELLED" => {
                self.finish_cancelled(job_id, workspace.as_ref(), &hooks, &mut on_event)
            }
            Err(error) => {
                self.finish_failed(job_id, workspace.as_ref(), &hooks, &error, &mut on_event)?;
                Err(error)
            }
        };
        self.state.cancellations.unregister(job_id);
        result
    }

    fn execute_inner<F>(
        &self,
        job_id: &str,
        token: &CancellationToken,
        hooks: &DiagnosticCopyHooks,
        workspace: &mut Option<JobWorkspace>,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let inspecting = self.transition(
            job_id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )?;
        emit(
            &inspecting,
            OperationStage::Inspect,
            "INSPECTING_INPUT",
            "Checking the selected file",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Inspect)?;

        let input = inspecting.inputs.first().ok_or_else(inspect_error)?.clone();
        let input_path = PathBuf::from(&input.canonical_path);
        let (revalidated_input, identity) =
            canonical_regular_file(&input_path).map_err(|_| path_error())?;
        if identity.to_string() != input.file_identity {
            return Err(safe_error(
                "SOURCE_CHANGED",
                "The source file changed",
                "Select the file again before retrying the operation.",
                OperationStage::Inspect,
                true,
            ));
        }

        let preflight = self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        emit(
            &preflight,
            OperationStage::Preflight,
            "CHECKING_DESTINATION",
            "Checking destination safety and available space",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Preflight)?;
        let destination = canonical_directory(Path::new(&preflight.destination_directory))
            .map_err(|_| path_error())?;
        validate_output_name(&preflight.requested_output_name).map_err(|_| path_error())?;
        ensure_different_files(
            &revalidated_input,
            &destination.join(&preflight.requested_output_name),
        )
        .map_err(|_| path_error())?;
        let required = input.size_bytes.saturating_add(COPY_BUFFER_SIZE as u64);
        if available_bytes(self.state.workspaces.root()).map_err(|_| preflight_error())? < required
            || available_bytes(&destination).map_err(|_| preflight_error())? < required
        {
            return Err(safe_error(
                "INSUFFICIENT_SPACE",
                "There is not enough available space",
                "Choose a destination with more free space and try again.",
                OperationStage::Preflight,
                true,
            ));
        }

        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Estimate,
            0,
            input.size_bytes,
            "ESTIMATE_READY",
            "The copy size has been estimated",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Estimate)?;
        *workspace = Some(
            self.state
                .workspaces
                .create_job(job_id)
                .map_err(|_| workspace_error())?,
        );
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Plan,
            0,
            input.size_bytes,
            "PLAN_READY",
            "A private temporary workspace is ready",
            true,
            on_event,
        )?;
        let ready = self.transition(
            job_id,
            JobState::Preflight,
            JobState::Ready,
            OperationStage::Plan,
        )?;
        emit(
            &ready,
            OperationStage::Plan,
            "READY_TO_COPY",
            "The diagnostic copy is ready",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Plan)?;

        let running = self.transition(
            job_id,
            JobState::Ready,
            JobState::Running,
            OperationStage::Execute,
        )?;
        emit(
            &running,
            OperationStage::Execute,
            "COPY_STARTED",
            "Copying and checking the file",
            true,
            on_event,
        );
        let staging_path = workspace
            .as_ref()
            .ok_or_else(workspace_error)?
            .staging
            .join("output.staging");
        let (copied_size, copied_hash) = self.copy_source_to_staging(
            job_id,
            &revalidated_input,
            &input.file_identity,
            &staging_path,
            input.size_bytes,
            token,
            hooks,
            on_event,
        )?;

        let verifying = self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        emit(
            &verifying,
            OperationStage::Verify,
            "VERIFYING_COPY",
            "Reopening and verifying the temporary copy",
            true,
            on_event,
        );
        if hooks.corrupt_staging_before_verify {
            OpenOptions::new()
                .append(true)
                .open(&staging_path)
                .and_then(|mut file| file.write_all(b"verification-fault"))
                .map_err(|_| write_error())?;
        }
        check_cancelled(token, OperationStage::Verify)?;
        let (verified_size, verified_hash) =
            hash_file(&staging_path).map_err(|_| verify_error())?;
        if copied_size != verified_size || copied_hash != verified_hash {
            return Err(verify_error());
        }
        {
            let mut database = self.state.database();
            database
                .update_input_hash(job_id, &copied_hash)
                .map_err(|_| metadata_error())?;
            database
                .set_output_staging(
                    job_id,
                    &staging_path.to_string_lossy(),
                    copied_size,
                    &copied_hash,
                    &timestamp(),
                )
                .map_err(|_| metadata_error())?;
        }

        self.progress(
            job_id,
            JobState::Verifying,
            OperationStage::Publish,
            0,
            copied_size,
            "PREPARING_PUBLICATION",
            "Preparing a verified destination copy without replacing existing files",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Publish)?;

        let requested_name = verifying.requested_output_name.clone();
        let state_for_intent = self.state.clone();
        let token_for_commit = token.clone();
        let token_for_progress = token.clone();
        let create_collision_before_first_commit =
            hooks.create_collision_before_first_publication_commit;
        let mut collision_created = false;
        let result = publish_verified_staging_with_observer(
            PublicationContext {
                staging_path: &staging_path,
                input_path: &revalidated_input,
                destination_directory: &destination,
                requested_name: &requested_name,
                job_id,
            },
            || token.is_cancelled(),
            |completed, total| {
                let publication_state = if token_for_progress.commit_started() {
                    JobState::Publishing
                } else {
                    JobState::Verifying
                };
                self.progress(
                    job_id,
                    publication_state,
                    OperationStage::Publish,
                    completed,
                    total,
                    "COPYING_DESTINATION_PARTIAL",
                    "Copying the verified file into the destination",
                    publication_state == JobState::Verifying,
                    on_event,
                )
                .map_err(|_| std::io::Error::other("publication progress could not be stored"))
            },
            move |candidate| {
                if create_collision_before_first_commit && !collision_created {
                    fs::write(candidate, b"competing output").map_err(PublicationError::Io)?;
                    collision_created = true;
                }
                let publication_already_started = token_for_commit.commit_started();
                if !token_for_commit.try_begin_publication_commit() {
                    return Err(PublicationError::Cancelled);
                }
                let resolved_name = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        PublicationError::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "invalid name",
                        ))
                    })?;
                let publication_result = if publication_already_started {
                    state_for_intent.database().set_publication_intent(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        copied_size,
                        &copied_hash,
                    )
                } else {
                    state_for_intent.database().begin_publication(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        copied_size,
                        &copied_hash,
                    )
                };
                publication_result.map_err(|_| {
                    PublicationError::Io(std::io::Error::other(
                        "publication intent could not be stored",
                    ))
                })?;
                Ok(())
            },
        )
        .map_err(publication_error)?;

        let publishing = self.current_job(job_id)?;
        emit(
            &publishing,
            OperationStage::Publish,
            "PUBLICATION_COMMITTED",
            "The verified output was published without replacing an existing file",
            false,
            on_event,
        );

        self.state
            .database()
            .set_output_published(
                job_id,
                &result.resolved_name,
                &result.final_path.to_string_lossy(),
                result.size_bytes,
                &result.sha256,
            )
            .map_err(|_| metadata_error())?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Audit,
            result.size_bytes,
            result.size_bytes,
            "AUDIT_SAVED",
            "Verified publication metadata has been saved",
            false,
            on_event,
        )?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Cleanup,
            result.size_bytes,
            result.size_bytes,
            "CLEANING_TEMPORARY_DATA",
            "Removing private temporary data",
            false,
            on_event,
        )?;
        if hooks.fail_cleanup {
            return Err(cleanup_error());
        }
        self.state
            .workspaces
            .cleanup_job(job_id)
            .map_err(|_| cleanup_error())?;
        self.state
            .database()
            .clear_ephemeral_paths(job_id)
            .map_err(|_| metadata_error())?;
        let completed = self.transition(
            job_id,
            JobState::Publishing,
            JobState::Completed,
            OperationStage::Cleanup,
        )?;
        emit(
            &completed,
            OperationStage::Cleanup,
            "COPY_COMPLETED",
            "The verified copy is ready",
            false,
            on_event,
        );
        Ok(completed)
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_source_to_staging<F>(
        &self,
        job_id: &str,
        input_path: &Path,
        expected_identity: &str,
        staging_path: &Path,
        total: u64,
        token: &CancellationToken,
        hooks: &DiagnosticCopyHooks,
        on_event: &mut F,
    ) -> Result<(u64, String), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let mut source = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
            .open(input_path)
            .map_err(|_| inspect_error())?;
        if identity_from_file(&source)
            .map_err(|_| inspect_error())?
            .to_string()
            != expected_identity
        {
            return Err(safe_error(
                "SOURCE_CHANGED",
                "The source file changed",
                "Select the file again before retrying the operation.",
                OperationStage::Execute,
                true,
            ));
        }
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging_path)
            .map_err(|_| write_error())?;
        let mut hasher = Sha256::new();
        let mut completed = 0_u64;
        let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
        let mut last_emit = Instant::now() - Duration::from_millis(125);
        loop {
            check_cancelled(token, OperationStage::Execute)?;
            if hooks
                .fail_write_after_bytes
                .is_some_and(|threshold| completed >= threshold)
            {
                return Err(write_error());
            }
            let read = source.read(&mut buffer).map_err(|_| inspect_error())?;
            if read == 0 {
                break;
            }
            destination
                .write_all(&buffer[..read])
                .map_err(|_| write_error())?;
            hasher.update(&buffer[..read]);
            completed = completed.saturating_add(read as u64);
            if last_emit.elapsed() >= Duration::from_millis(125) || completed == total {
                self.progress(
                    job_id,
                    JobState::Running,
                    OperationStage::Execute,
                    completed,
                    total,
                    "COPYING_BYTES",
                    "Copying and checking the file",
                    true,
                    on_event,
                )?;
                last_emit = Instant::now();
            }
        }
        destination.sync_all().map_err(|_| write_error())?;
        if completed != total {
            return Err(safe_error(
                "SOURCE_CHANGED",
                "The source file changed while it was being read",
                "Select the file again before retrying the operation.",
                OperationStage::Execute,
                true,
            ));
        }
        let digest = hasher.finalize();
        Ok((completed, digest_hex(&digest)))
    }

    fn transition(
        &self,
        job_id: &str,
        expected: JobState,
        next: JobState,
        stage: OperationStage,
    ) -> Result<JobRecord, OperationError> {
        let mut database = self.state.database();
        let current = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        database
            .transition_job(job_id, expected, current.version, next, Some(stage))
            .map_err(|_| metadata_error())?;
        database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)
    }

    #[allow(clippy::too_many_arguments)]
    fn progress<F>(
        &self,
        job_id: &str,
        state: JobState,
        stage: OperationStage,
        completed: u64,
        total: u64,
        code: &str,
        message: &str,
        cancellable: bool,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let job = {
            let mut database = self.state.database();
            database
                .update_progress(job_id, state, stage, completed, total, ProgressUnit::Bytes)
                .map_err(|_| metadata_error())?;
            database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?
        };
        emit(&job, stage, code, message, cancellable, on_event);
        Ok(())
    }

    fn finish_cancelled<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        hooks: &DiagnosticCopyHooks,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        if hooks.fail_cleanup
            || (workspace.is_some() && self.state.workspaces.cleanup_job(job_id).is_err())
        {
            let current = self.current_job(job_id)?;
            self.state
                .database()
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            return Err(cleanup_error());
        }
        let mut database = self.state.database();
        database
            .clear_ephemeral_paths(job_id)
            .map_err(|_| metadata_error())?;
        let current = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        database
            .transition_job(
                job_id,
                current.state,
                current.version,
                JobState::Cancelled,
                Some(OperationStage::Cleanup),
            )
            .map_err(|_| metadata_error())?;
        let cancelled = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        emit(
            &cancelled,
            OperationStage::Cleanup,
            "COPY_CANCELLED",
            "The diagnostic copy was cancelled and temporary data was removed",
            false,
            on_event,
        );
        Ok(cancelled)
    }

    fn finish_failed<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        hooks: &DiagnosticCopyHooks,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let current = self.current_job(job_id)?;
        let cleanup_failed = hooks.fail_cleanup
            || (workspace.is_some() && self.state.workspaces.cleanup_job(job_id).is_err());
        let mut database = self.state.database();
        database
            .record_error(job_id, error)
            .map_err(|_| metadata_error())?;
        if cleanup_failed || current.state == JobState::Publishing {
            if !cleanup_failed {
                database
                    .clear_ephemeral_paths(job_id)
                    .map_err(|_| metadata_error())?;
            }
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            return Ok(());
        }
        database
            .clear_ephemeral_paths(job_id)
            .map_err(|_| metadata_error())?;
        let refreshed = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        database
            .transition_job(
                job_id,
                refreshed.state,
                refreshed.version,
                JobState::Failed,
                Some(OperationStage::Cleanup),
            )
            .map_err(|_| metadata_error())?;
        let failed = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        emit(
            &failed,
            OperationStage::Cleanup,
            "COPY_FAILED",
            "The diagnostic copy failed and temporary data was removed",
            false,
            on_event,
        );
        Ok(())
    }

    fn current_job(&self, job_id: &str) -> Result<JobRecord, OperationError> {
        self.state
            .database()
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)
    }
}

fn emit<F>(
    job: &JobRecord,
    stage: OperationStage,
    message_code: &str,
    message: &str,
    cancellable: bool,
    on_event: &mut F,
) where
    F: FnMut(ProgressEvent),
{
    on_event(ProgressEvent {
        schema_version: 1,
        sequence: job.sequence,
        emitted_at: timestamp(),
        job_id: job.id.clone(),
        operation_id: job.operation_id.clone(),
        state: job.state,
        stage,
        completed_units: job.progress.completed_units,
        total_units: job.progress.total_units,
        unit: job.progress.unit,
        message_code: message_code.to_owned(),
        message: message.to_owned(),
        cancellable,
    });
}

fn check_cancelled(token: &CancellationToken, stage: OperationStage) -> Result<(), OperationError> {
    if token.is_cancelled() {
        return Err(safe_error(
            "CANCELLED",
            "The operation was cancelled",
            "Temporary data will be removed before the job is marked cancelled.",
            stage,
            false,
        ));
    }
    Ok(())
}

fn safe_error(
    code: &str,
    title: &str,
    detail: &str,
    stage: OperationStage,
    retryable: bool,
) -> OperationError {
    OperationError::safe(code, title, detail, stage, retryable)
}

fn path_error() -> OperationError {
    safe_error(
        "PATH_UNSAFE",
        "The selected path is not safe",
        "Choose a regular local file and a local destination without links or special path syntax.",
        OperationStage::Preflight,
        false,
    )
}

fn inspect_error() -> OperationError {
    safe_error(
        "INPUT_UNREADABLE",
        "The selected file could not be read",
        "Check that the file still exists and that Document Studio can read it.",
        OperationStage::Inspect,
        true,
    )
}

fn preflight_error() -> OperationError {
    safe_error(
        "PREFLIGHT_FAILED",
        "The destination could not be prepared",
        "Check destination access and available space, then try again.",
        OperationStage::Preflight,
        true,
    )
}

fn workspace_error() -> OperationError {
    safe_error(
        "WORKSPACE_FAILED",
        "A private temporary workspace could not be prepared",
        "Close other instances and try the operation again.",
        OperationStage::Plan,
        true,
    )
}

fn write_error() -> OperationError {
    safe_error(
        "WRITE_FAILED",
        "The temporary copy could not be written",
        "Check available space and permissions, then try again.",
        OperationStage::Execute,
        true,
    )
}

fn verify_error() -> OperationError {
    safe_error(
        "VERIFICATION_MISMATCH",
        "The copied file did not pass verification",
        "No output was published. Retry the operation from the original file.",
        OperationStage::Verify,
        true,
    )
}

fn publication_error(error: PublicationError) -> OperationError {
    match error {
        PublicationError::Cancelled => safe_error(
            "CANCELLED",
            "The operation was cancelled",
            "The destination partial will be removed before cancellation is recorded.",
            OperationStage::Publish,
            false,
        ),
        PublicationError::VerificationMismatch => verify_error(),
        PublicationError::CollisionExhausted => safe_error(
            "OUTPUT_COLLISION_LIMIT",
            "A safe output name could not be reserved",
            "Choose a different output name or destination and try again.",
            OperationStage::Publish,
            true,
        ),
        PublicationError::InsufficientSpace => safe_error(
            "INSUFFICIENT_SPACE",
            "There is not enough available space",
            "Choose a destination with more free space and try again.",
            OperationStage::Publish,
            true,
        ),
        PublicationError::Path(_) => path_error(),
        PublicationError::Io(_) => safe_error(
            "PUBLICATION_FAILED",
            "The verified copy could not be published",
            "No existing file was replaced. Check destination access and try again.",
            OperationStage::Publish,
            true,
        ),
    }
}

fn cleanup_error() -> OperationError {
    safe_error(
        "CLEANUP_FAILED",
        "Temporary data could not be completely removed",
        "The job remains interrupted so cleanup can be retried safely at startup.",
        OperationStage::Cleanup,
        true,
    )
}

fn metadata_error() -> OperationError {
    safe_error(
        "METADATA_WRITE_FAILED",
        "Job metadata could not be saved",
        "The operation cannot report success until its audit metadata is safely stored.",
        OperationStage::Audit,
        true,
    )
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl From<DatabaseError> for OperationError {
    fn from(_value: DatabaseError) -> Self {
        metadata_error()
    }
}
