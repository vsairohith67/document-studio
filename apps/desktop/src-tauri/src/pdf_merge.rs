use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ};

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    JobInput, JobOutput, JobProgress, JobRecord, JobState, JobsCreateRequest, OperationError,
    OperationStage, OutputStatus, ProgressEvent, ProgressUnit, PDF_MERGE_MAX_INPUTS,
    PDF_MERGE_MIN_INPUTS, PDF_MERGE_OPERATION_ID, PDF_MERGE_VERSION,
};
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
    PathPolicyError,
};
use crate::process_sandbox::{
    authorize_qpdf_paths, ensure_production_profile, spawn_sandboxed, SandboxError,
    SandboxExecution, SandboxLaunchSpec, QPDF_PROCESS_TIMEOUT,
};
use crate::publication::{
    hash_file, is_exact_owned_partial_path, partial_ownership_result_code,
    publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::qpdf::{
    build_production_merge_arguments, interpret_encryption_check_exit,
    interpret_structural_check_exit, EncryptionCheckOutcome, OrdinalSnapshot,
    StructuralCheckOutcome, VerifiedQpdfRuntime, MERGED_STAGING_RELATIVE_PATH,
};
use crate::windows_security::{
    available_bytes, delete_open_file, identity_from_file, open_for_identity_and_delete,
};
use crate::workspace::JobWorkspace;

pub const PREFLIGHT_MAX_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniquePreflightSource {
    pub file_identity: String,
    pub ordinals: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfMergeInputPlan {
    pub unique_sources: Vec<UniquePreflightSource>,
    pub snapshots: Vec<OrdinalSnapshot>,
    pub preflight_concurrency: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PdfMergePlanError {
    #[error("PDF Merge requires between 2 and 128 inputs")]
    InputCount,
    #[error("input ordinals must be contiguous and ordered")]
    OrdinalOrder,
    #[error("every planned input must be a PDF")]
    MimeType,
}

pub fn plan_inputs(inputs: &[JobInput]) -> Result<PdfMergeInputPlan, PdfMergePlanError> {
    if !(PDF_MERGE_MIN_INPUTS..=PDF_MERGE_MAX_INPUTS).contains(&inputs.len()) {
        return Err(PdfMergePlanError::InputCount);
    }

    let mut identity_indexes = HashMap::<&str, usize>::new();
    let mut unique_sources = Vec::<UniquePreflightSource>::new();
    let mut snapshots = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let ordinal = u32::try_from(index).map_err(|_| PdfMergePlanError::OrdinalOrder)?;
        if input.ordinal != ordinal {
            return Err(PdfMergePlanError::OrdinalOrder);
        }
        if input.mime_type != "application/pdf" {
            return Err(PdfMergePlanError::MimeType);
        }

        if let Some(unique_index) = identity_indexes.get(input.file_identity.as_str()) {
            unique_sources[*unique_index].ordinals.push(ordinal);
        } else {
            identity_indexes.insert(&input.file_identity, unique_sources.len());
            unique_sources.push(UniquePreflightSource {
                file_identity: input.file_identity.clone(),
                ordinals: vec![ordinal],
            });
        }
        snapshots.push(OrdinalSnapshot::for_ordinal(ordinal));
    }

    let available = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let preflight_concurrency = unique_sources
        .len()
        .min(PREFLIGHT_MAX_CONCURRENCY)
        .min(available)
        .max(1);
    Ok(PdfMergeInputPlan {
        unique_sources,
        snapshots,
        preflight_concurrency,
    })
}

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const MIN_PDF_SIZE: u64 = 8;
const SPACE_MARGIN_MINIMUM: u64 = 64 * 1024 * 1024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const PREFLIGHT_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct PdfMergeHooks {
    pub fail_snapshot_after_bytes: Option<u64>,
    pub corrupt_staging_before_verify: bool,
    pub create_collision_before_first_publication_commit: bool,
    pub pause_before_merge: Option<Duration>,
    pub pause_after_merge_process_start: Option<Duration>,
    pub merge_process_started: Option<Arc<AtomicBool>>,
    pub merge_process_peak_memory: Option<Arc<Mutex<Vec<usize>>>>,
    pub pause_before_publication_commit: Option<Duration>,
    pub fail_cleanup: bool,
    pub available_space_override: Option<u64>,
}

#[derive(Clone)]
pub struct PdfMergeService {
    state: AppState,
}

#[derive(Debug, Clone)]
struct PreflightResult {
    identity: String,
    page_count: u64,
}

impl PdfMergeService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn create_job(&self, request: JobsCreateRequest) -> Result<JobRecord, OperationError> {
        if request.operation_id != PDF_MERGE_OPERATION_ID
            || !(PDF_MERGE_MIN_INPUTS..=PDF_MERGE_MAX_INPUTS).contains(&request.input_paths.len())
        {
            return Err(invalid_request());
        }
        validate_output_name(&request.requested_output_name).map_err(|_| path_error())?;
        if !request
            .requested_output_name
            .to_ascii_lowercase()
            .ends_with(".pdf")
        {
            return Err(invalid_request());
        }
        let destination = canonical_directory(Path::new(&request.destination_directory))
            .map_err(|_| path_error())?;
        let requested_path = destination.join(&request.requested_output_name);
        let mut inputs = Vec::with_capacity(request.input_paths.len());
        let mut total_bytes = 0_u64;
        for (index, original) in request.input_paths.iter().enumerate() {
            let ordinal = u32::try_from(index).map_err(|_| invalid_request())?;
            let original_path = Path::new(original);
            if !has_pdf_extension(original_path) {
                return Err(input_error(
                    "INPUT_NOT_PDF",
                    "The selected file is not a PDF",
                    "Choose a file with a .pdf extension and a valid PDF header.",
                    OperationStage::Inspect,
                    false,
                    index,
                ));
            }
            let (canonical, expected_identity) = canonical_regular_file(original_path)
                .map_err(|error| canonical_input_error(error, index, OperationStage::Inspect))?;
            ensure_different_files(&canonical, &requested_path)
                .map_err(|_| destination_input_error(index))?;
            let mut source = open_source(&canonical, OperationStage::Inspect, index)?;
            let identity = identity_from_file(&source).map_err(|_| inspect_error(index))?;
            if identity != expected_identity
                || !pdf_magic(&mut source).map_err(|_| inspect_error(index))?
            {
                return Err(input_error(
                    "INPUT_NOT_PDF",
                    "The selected file is not a PDF",
                    "Choose a file with a .pdf extension and a valid PDF header.",
                    OperationStage::Inspect,
                    false,
                    index,
                ));
            }
            let metadata = source.metadata().map_err(|_| inspect_error(index))?;
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| size_error(OperationStage::Inspect))?;
            let display_name = canonical
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| path_error_for(index))?
                .to_owned();
            inputs.push(JobInput {
                ordinal,
                display_name,
                source_path: original.clone(),
                canonical_path: canonical.to_string_lossy().into_owned(),
                file_identity: identity.to_string(),
                size_bytes: metadata.len(),
                modified_at: modified_timestamp(&metadata).map_err(|_| inspect_error(index))?,
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                password_reference: None,
            });
        }
        let now = timestamp();
        let job = JobRecord {
            id: Uuid::new_v4().hyphenated().to_string(),
            operation_id: PDF_MERGE_OPERATION_ID.to_owned(),
            operation_version: PDF_MERGE_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: total_bytes,
                unit: ProgressUnit::Bytes,
            },
            destination_directory: destination.to_string_lossy().into_owned(),
            requested_output_name: request.requested_output_name.clone(),
            resolved_output_name: None,
            cancellation_requested_at: None,
            created_at: now.clone(),
            updated_at: now,
            finished_at: None,
            version: 0,
            inputs,
            outputs: vec![JobOutput {
                ordinal: 0,
                requested_name: request.requested_output_name,
                resolved_name: None,
                staging_path: None,
                partial_path: None,
                final_path: None,
                size_bytes: None,
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                status: OutputStatus::Planned,
                verified_at: None,
                published_at: None,
            }],
            errors: Vec::new(),
        };
        plan_inputs(&job.inputs).map_err(|_| invalid_request())?;
        self.state
            .database()
            .create_job(&job)
            .map_err(|_| metadata_error())?;
        Ok(job)
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
        self.execute_with_registered_token_and_hooks(
            job_id,
            token,
            on_event,
            PdfMergeHooks::default(),
        )
    }

    pub fn execute<F>(&self, job_id: &str, on_event: F) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let token = self.state.cancellations.register(job_id);
        self.execute_with_registered_token_and_hooks(
            job_id,
            token,
            on_event,
            PdfMergeHooks::default(),
        )
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
        let token = self.state.cancellations.register(job_id);
        self.execute_with_registered_token_and_hooks(job_id, token, on_event, hooks)
    }

    fn execute_with_registered_token_and_hooks<F>(
        &self,
        job_id: &str,
        token: CancellationToken,
        mut on_event: F,
        hooks: PdfMergeHooks,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let mut workspace = None;
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
        hooks: &PdfMergeHooks,
        workspace_slot: &mut Option<JobWorkspace>,
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
            "INSPECTING_PDFS",
            "Checking the ordered PDF list",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Inspect)?;
        let plan = plan_inputs(&inspecting.inputs).map_err(|_| invalid_request())?;
        let mut canonical_inputs = Vec::with_capacity(inspecting.inputs.len());
        for (index, input) in inspecting.inputs.iter().enumerate() {
            let path = PathBuf::from(&input.canonical_path);
            let (canonical, identity) = canonical_regular_file(&path)
                .map_err(|error| canonical_input_error(error, index, OperationStage::Inspect))?;
            let metadata = fs::metadata(&canonical).map_err(|_| inspect_error(index))?;
            if identity.to_string() != input.file_identity
                || metadata.len() != input.size_bytes
                || modified_timestamp(&metadata).map_err(|_| inspect_error(index))?
                    != input.modified_at
            {
                return Err(source_changed(index, OperationStage::Inspect));
            }
            canonical_inputs.push(canonical);
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
            "CHECKING_MERGE_SAFETY",
            "Checking local space, destination safety and the bundled PDF engine",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Preflight)?;
        let destination = canonical_directory(Path::new(&preflight.destination_directory))
            .map_err(|_| path_error())?;
        validate_output_name(&preflight.requested_output_name).map_err(|_| path_error())?;
        let requested_path = destination.join(&preflight.requested_output_name);
        for (index, input) in canonical_inputs.iter().enumerate() {
            ensure_different_files(input, &requested_path)
                .map_err(|_| destination_input_error(index))?;
        }
        let ordered_bytes = preflight
            .inputs
            .iter()
            .try_fold(0_u64, |sum, input| sum.checked_add(input.size_bytes))
            .ok_or_else(|| size_error(OperationStage::Estimate))?;
        let margin = SPACE_MARGIN_MINIMUM.max(ordered_bytes / 10);
        let workspace_required = ordered_bytes
            .checked_mul(2)
            .and_then(|value| value.checked_add(margin))
            .ok_or_else(|| size_error(OperationStage::Estimate))?;
        let destination_required = ordered_bytes
            .checked_add(margin)
            .ok_or_else(|| size_error(OperationStage::Estimate))?;
        let workspace_available = hooks.available_space_override.unwrap_or(
            available_bytes(self.state.workspaces.root()).map_err(|_| preflight_error())?,
        );
        let destination_available = hooks
            .available_space_override
            .unwrap_or(available_bytes(&destination).map_err(|_| preflight_error())?);
        if workspace_available < workspace_required || destination_available < destination_required
        {
            return Err(insufficient_space(OperationStage::Preflight));
        }
        let runtime = self
            .state
            .qpdf
            .as_ref()
            .ok_or_else(dependency_error)?
            .get_or_prepare()
            .map_err(|_| dependency_error())?;

        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Estimate,
            0,
            ordered_bytes,
            ProgressUnit::Bytes,
            "MERGE_ESTIMATE_READY",
            "The private workspace and destination budget have been checked",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Estimate)?;
        let workspace = self
            .state
            .workspaces
            .create_job(job_id)
            .map_err(|_| workspace_error())?;
        let profile = ensure_production_profile().map_err(|_| dependency_error())?;
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| dependency_error())?;
        verify_qpdf_version(&runtime, &workspace, token)?;
        *workspace_slot = Some(workspace.clone());
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Plan,
            0,
            u64::try_from(preflight.inputs.len()).unwrap_or(0),
            ProgressUnit::Items,
            "MERGE_PLAN_READY",
            "The exact persisted input order is ready",
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
            "READY_TO_MERGE",
            "PDF Merge is ready to freeze the selected inputs",
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
            "SNAPSHOTTING_PDFS",
            "Freezing one private snapshot for each ordered input",
            true,
            on_event,
        );

        let mut completed_bytes = 0_u64;
        for (index, input) in running.inputs.iter().enumerate() {
            let snapshot = workspace.root.join(&plan.snapshots[index].relative_path);
            let (size, hash) = self.copy_ordinal_snapshot(
                job_id,
                input,
                &snapshot,
                ordered_bytes,
                &mut completed_bytes,
                token,
                hooks,
                on_event,
            )?;
            if size != input.size_bytes {
                return Err(source_changed(index, OperationStage::Execute));
            }
            self.state
                .database()
                .update_input_hash(job_id, input.ordinal, &hash)
                .map_err(|_| metadata_error())?;
        }
        check_cancelled(token, OperationStage::Preflight)?;
        self.progress(
            job_id,
            JobState::Running,
            OperationStage::Preflight,
            0,
            u64::try_from(plan.unique_sources.len()).unwrap_or(0),
            ProgressUnit::Items,
            "VALIDATING_PDFS",
            "Strictly checking each unique PDF source",
            true,
            on_event,
        )?;
        let preflight_results = preflight_unique_sources(
            &runtime,
            &workspace,
            &plan,
            token,
            plan.preflight_concurrency,
        )?;
        let pages_by_identity = preflight_results
            .into_iter()
            .map(|result| (result.identity, result.page_count))
            .collect::<HashMap<_, _>>();
        let expected_pages = running
            .inputs
            .iter()
            .try_fold(0_u64, |sum, input| {
                pages_by_identity
                    .get(&input.file_identity)
                    .and_then(|pages| sum.checked_add(*pages))
            })
            .ok_or_else(|| verify_error("PAGE_COUNT_INVALID"))?;

        if let Some(pause) = hooks.pause_before_merge {
            cancellable_pause(token, pause, OperationStage::Execute)?;
        }
        let arguments = build_production_merge_arguments(
            &plan.snapshots,
            Path::new(MERGED_STAGING_RELATIVE_PATH),
        )
        .map_err(|_| process_error(OperationStage::Execute))?;
        verify_ordinal_arguments(&arguments, &plan.snapshots)?;
        self.progress(
            job_id,
            JobState::Running,
            OperationStage::Execute,
            0,
            u64::try_from(running.inputs.len()).unwrap_or(0),
            ProgressUnit::Items,
            "MERGING_PDFS",
            "Merging the ordered PDF snapshots",
            true,
            on_event,
        )?;
        let execution = run_qpdf_after_start_pause(
            &runtime,
            &workspace,
            &arguments,
            token,
            QPDF_PROCESS_TIMEOUT,
            OperationStage::Execute,
            QpdfStartObservation {
                pause: hooks.pause_after_merge_process_start,
                started: hooks.merge_process_started.as_ref(),
            },
        )?;
        if let Some(observed) = &hooks.merge_process_peak_memory {
            observed
                .lock()
                .map_err(|_| process_error(OperationStage::Execute))?
                .push(execution.peak_process_memory_bytes);
        }
        if execution.exit_code != 0 {
            return Err(process_error(OperationStage::Execute));
        }
        let staging_path = workspace.root.join(MERGED_STAGING_RELATIVE_PATH);
        if hooks.corrupt_staging_before_verify {
            let file = OpenOptions::new()
                .write(true)
                .open(&staging_path)
                .map_err(|_| verify_error("OUTPUT_VERIFICATION_FAILED"))?;
            file.set_len(4)
                .map_err(|_| verify_error("OUTPUT_VERIFICATION_FAILED"))?;
        }

        let verifying = self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        emit(
            &verifying,
            OperationStage::Verify,
            "VERIFYING_MERGED_PDF",
            "Independently reopening and verifying the merged PDF",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Verify)?;
        let (verified_size, verified_hash) =
            verify_output(&runtime, &workspace, &staging_path, expected_pages, token)?;
        self.state
            .database()
            .set_output_staging(
                job_id,
                &staging_path.to_string_lossy(),
                verified_size,
                &verified_hash,
                &timestamp(),
            )
            .map_err(|_| metadata_error())?;
        if available_bytes(&destination).map_err(|_| preflight_error())?
            < verified_size.saturating_add(COPY_BUFFER_SIZE as u64)
        {
            return Err(insufficient_space(OperationStage::Publish));
        }

        self.progress(
            job_id,
            JobState::Verifying,
            OperationStage::Publish,
            0,
            verified_size,
            ProgressUnit::Bytes,
            "PREPARING_PUBLICATION",
            "Preparing a verified destination copy without replacing existing files",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Publish)?;
        let input_refs = canonical_inputs
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        let requested_name = verifying.requested_output_name.clone();
        let state_for_reservation = self.state.clone();
        let state_for_activation = self.state.clone();
        let destination_for_activation = destination.clone();
        let state_for_release = self.state.clone();
        let state_for_intent = self.state.clone();
        let token_for_commit = token.clone();
        let token_for_progress = token.clone();
        let pause_before_commit = hooks.pause_before_publication_commit;
        let create_collision = hooks.create_collision_before_first_publication_commit;
        let mut collision_created = false;
        let result = publish_verified_staging_with_observer(
            PublicationContext {
                staging_path: &staging_path,
                input_paths: &input_refs,
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
                    ProgressUnit::Bytes,
                    "COPYING_DESTINATION_PARTIAL",
                    "Copying the verified PDF into the destination",
                    publication_state == JobState::Verifying,
                    on_event,
                )
                .map_err(|_| std::io::Error::other("publication progress could not be stored"))
            },
            move |candidate, partial, resolved_name, size, sha256| {
                state_for_reservation
                    .database()
                    .reserve_publication_attempt(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        &partial.to_string_lossy(),
                        size,
                        sha256,
                    )
                    .map_err(|_| {
                        PublicationError::Io(std::io::Error::other(
                            "publication ownership could not be stored",
                        ))
                    })
            },
            move |partial, identity| {
                let ownership = partial_ownership_result_code(
                    &destination_for_activation,
                    job_id,
                    partial,
                    identity,
                )
                .ok_or_else(|| {
                    PublicationError::Io(std::io::Error::other(
                        "publication ownership proof is invalid",
                    ))
                })?;
                state_for_activation
                    .database()
                    .activate_owned_partial(job_id, &partial.to_string_lossy(), &ownership)
                    .map_err(|_| {
                        PublicationError::Io(std::io::Error::other(
                            "publication ownership could not be activated",
                        ))
                    })
            },
            move |partial| {
                state_for_release
                    .database()
                    .clear_owned_partial(job_id, &partial.to_string_lossy())
                    .map_err(|_| {
                        PublicationError::Io(std::io::Error::other(
                            "publication ownership could not be released",
                        ))
                    })
            },
            move |candidate| {
                if create_collision && !collision_created {
                    fs::write(candidate, b"competing output").map_err(PublicationError::Io)?;
                    collision_created = true;
                }
                if let Some(pause) = pause_before_commit {
                    cancellable_pause(&token_for_commit, pause, OperationStage::Publish)
                        .map_err(|_| PublicationError::Cancelled)?;
                }
                let already_started = token_for_commit.commit_started();
                if !token_for_commit.try_begin_publication_commit() {
                    return Err(PublicationError::Cancelled);
                }
                let resolved_name = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        PublicationError::Io(std::io::Error::other("invalid output name"))
                    })?;
                let write = if already_started {
                    state_for_intent.database().set_publication_intent(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        verified_size,
                        &verified_hash,
                    )
                } else {
                    state_for_intent.database().begin_publication(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        verified_size,
                        &verified_hash,
                    )
                };
                write.map_err(|_| {
                    PublicationError::Io(std::io::Error::other(
                        "publication intent could not be stored",
                    ))
                })
            },
        )
        .map_err(publication_error)?;

        let publishing = self.current_job(job_id)?;
        emit(
            &publishing,
            OperationStage::Publish,
            "PUBLICATION_COMMITTED",
            "The verified PDF was published without replacing an existing file",
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
                Some(&result.owned_partial_path.to_string_lossy()),
            )
            .map_err(|_| metadata_error())?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Audit,
            result.size_bytes,
            result.size_bytes,
            ProgressUnit::Bytes,
            "MERGE_AUDIT_SAVED",
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
            ProgressUnit::Bytes,
            "CLEANING_TEMPORARY_DATA",
            "Removing private PDF snapshots and temporary data",
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
            .clear_staging_path(job_id, Some(&staging_path.to_string_lossy()))
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
            "MERGE_COMPLETED",
            "The verified merged PDF is ready",
            false,
            on_event,
        );
        Ok(completed)
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_ordinal_snapshot<F>(
        &self,
        job_id: &str,
        input: &JobInput,
        destination_path: &Path,
        total_bytes: u64,
        completed_bytes: &mut u64,
        token: &CancellationToken,
        hooks: &PdfMergeHooks,
        on_event: &mut F,
    ) -> Result<(u64, String), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let index = input.ordinal as usize;
        let source_path = Path::new(&input.canonical_path);
        let mut source = open_source(source_path, OperationStage::Execute, index)?;
        if identity_from_file(&source)
            .map_err(|_| inspect_error(index))?
            .to_string()
            != input.file_identity
        {
            return Err(source_changed(index, OperationStage::Execute));
        }
        let before = source.metadata().map_err(|_| inspect_error(index))?;
        if before.len() != input.size_bytes
            || modified_timestamp(&before).map_err(|_| inspect_error(index))? != input.modified_at
        {
            return Err(source_changed(index, OperationStage::Execute));
        }
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination_path)
            .map_err(|_| write_error())?;
        let mut hash = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
        let mut last_emit = Instant::now() - Duration::from_millis(125);
        loop {
            check_cancelled(token, OperationStage::Execute)?;
            if hooks
                .fail_snapshot_after_bytes
                .is_some_and(|threshold| completed_bytes.saturating_add(copied) >= threshold)
            {
                return Err(write_error());
            }
            let read = source.read(&mut buffer).map_err(|_| inspect_error(index))?;
            if read == 0 {
                break;
            }
            destination
                .write_all(&buffer[..read])
                .map_err(|_| write_error())?;
            hash.update(&buffer[..read]);
            copied = copied.saturating_add(read as u64);
            if last_emit.elapsed() >= Duration::from_millis(125) || copied == input.size_bytes {
                self.progress(
                    job_id,
                    JobState::Running,
                    OperationStage::Execute,
                    completed_bytes.saturating_add(copied),
                    total_bytes,
                    ProgressUnit::Bytes,
                    "SNAPSHOTTING_BYTES",
                    "Freezing ordered PDF snapshots",
                    true,
                    on_event,
                )?;
                last_emit = Instant::now();
            }
        }
        destination.sync_all().map_err(|_| write_error())?;
        let after = source.metadata().map_err(|_| inspect_error(index))?;
        if copied != input.size_bytes
            || after.len() != before.len()
            || after.modified().map_err(|_| inspect_error(index))?
                != before.modified().map_err(|_| inspect_error(index))?
        {
            return Err(source_changed(index, OperationStage::Execute));
        }
        *completed_bytes = completed_bytes.saturating_add(copied);
        Ok((copied, digest_hex(&hash.finalize())))
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
        unit: ProgressUnit,
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
                .update_progress(job_id, state, stage, completed, total, unit)
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
        hooks: &PdfMergeHooks,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        if let Err(cleanup) = self.reconcile_temporary_artifacts(job_id, workspace, hooks) {
            let current = self.current_job(job_id)?;
            let mut database = self.state.database();
            database
                .record_error_once(job_id, &cleanup)
                .map_err(|_| metadata_error())?;
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            return Err(cleanup_error());
        }
        let mut database = self.state.database();
        database
            .clear_unpublished_intent(job_id)
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
            "MERGE_CANCELLED",
            "PDF Merge was cancelled and temporary data was removed",
            false,
            on_event,
        );
        Ok(cancelled)
    }

    fn finish_failed<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        hooks: &PdfMergeHooks,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let current = self.current_job(job_id)?;
        let cleanup = self.reconcile_temporary_artifacts(job_id, workspace, hooks);
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        if let Err(cleanup_error_value) = cleanup {
            database
                .record_error_once(job_id, &cleanup_error_value)
                .map_err(|_| metadata_error())?;
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            let interrupted = database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?;
            emit(
                &interrupted,
                OperationStage::Recovery,
                "MERGE_INTERRUPTED",
                "PDF Merge stopped with owned cleanup still pending",
                false,
                on_event,
            );
            return Ok(());
        }
        if current.state == JobState::Publishing {
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            let interrupted = database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?;
            emit(
                &interrupted,
                OperationStage::Recovery,
                "MERGE_INTERRUPTED",
                "PDF Merge publication needs evidence-based recovery",
                false,
                on_event,
            );
            return Ok(());
        }
        database
            .clear_unpublished_intent(job_id)
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
            "MERGE_FAILED",
            "PDF Merge failed and temporary data was removed",
            false,
            on_event,
        );
        Ok(())
    }

    fn reconcile_temporary_artifacts(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        hooks: &PdfMergeHooks,
    ) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        let output = job.outputs.first().ok_or_else(metadata_error)?;
        if let Some(partial) = output.partial_path.as_deref() {
            let partial = Path::new(partial);
            let destination = Path::new(&job.destination_directory);
            if !is_exact_owned_partial_path(destination, job_id, partial) || hooks.fail_cleanup {
                return Err(cleanup_error());
            }
            match open_for_identity_and_delete(partial) {
                Ok(file) => {
                    let identity = identity_from_file(&file).map_err(|_| cleanup_error())?;
                    let ownership =
                        partial_ownership_result_code(destination, job_id, partial, identity)
                            .ok_or_else(cleanup_error)?;
                    if self
                        .state
                        .database()
                        .owned_partial_is_activated(job_id, &partial.to_string_lossy(), &ownership)
                        .map_err(|_| metadata_error())?
                    {
                        delete_open_file(file).map_err(|_| cleanup_error())?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(cleanup_error()),
            }
            self.state
                .database()
                .clear_owned_partial(job_id, &partial.to_string_lossy())
                .map_err(|_| metadata_error())?;
        }
        if hooks.fail_cleanup {
            return Err(cleanup_error());
        }
        if workspace.is_some() || output.staging_path.is_some() {
            self.state
                .workspaces
                .cleanup_job(job_id)
                .map_err(|_| cleanup_error())?;
        }
        self.state
            .database()
            .clear_staging_path(job_id, output.staging_path.as_deref())
            .map_err(|_| metadata_error())?;
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

fn preflight_unique_sources(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    plan: &PdfMergeInputPlan,
    token: &CancellationToken,
    concurrency: usize,
) -> Result<Vec<PreflightResult>, OperationError> {
    let mut results = Vec::with_capacity(plan.unique_sources.len());
    for batch in plan.unique_sources.chunks(concurrency.max(1)) {
        let batch_results =
            std::thread::scope(|scope| {
                let mut workers = Vec::with_capacity(batch.len());
                for source in batch {
                    let runtime = runtime.clone();
                    let workspace = workspace.clone();
                    let token = token.clone();
                    let source = source.clone();
                    workers.push(scope.spawn(move || {
                        preflight_unique_source(&runtime, &workspace, &source, &token)
                    }));
                }
                workers
                    .into_iter()
                    .map(|worker| {
                        worker
                            .join()
                            .map_err(|_| process_error(OperationStage::Preflight))?
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?;
        results.extend(batch_results);
        check_cancelled(token, OperationStage::Preflight)?;
    }
    Ok(results)
}

fn preflight_unique_source(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    source: &UniquePreflightSource,
    token: &CancellationToken,
) -> Result<PreflightResult, OperationError> {
    let ordinal = *source.ordinals.first().ok_or_else(invalid_request)?;
    let relative = crate::qpdf::snapshot_relative_path(ordinal);
    let input_index = ordinal as usize;
    let encryption = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        PREFLIGHT_PROCESS_TIMEOUT,
        OperationStage::Preflight,
    )?;
    match interpret_encryption_check_exit(encryption.exit_code as i32) {
        Ok(EncryptionCheckOutcome::Encrypted) => {
            return Err(input_error(
                "PDF_ENCRYPTED",
                "The PDF is encrypted",
                "PDF Merge does not accept password-protected or restriction-encrypted files.",
                OperationStage::Preflight,
                false,
                input_index,
            ));
        }
        Ok(EncryptionCheckOutcome::Unencrypted) => {}
        Err(_) => return Err(process_error(OperationStage::Preflight)),
    }
    let structural = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--check"),
        ],
        token,
        PREFLIGHT_PROCESS_TIMEOUT,
        OperationStage::Preflight,
    )?;
    match interpret_structural_check_exit(structural.exit_code as i32) {
        Ok(StructuralCheckOutcome::Valid) => {}
        Ok(StructuralCheckOutcome::Rejected) => {
            if matches!(
                qpdf_page_count(
                    runtime,
                    workspace,
                    &relative,
                    token,
                    OperationStage::Preflight,
                ),
                Ok(0)
            ) {
                return Err(input_error(
                    "PDF_ZERO_PAGES",
                    "The PDF has no pages",
                    "Choose a PDF with at least one page.",
                    OperationStage::Preflight,
                    false,
                    input_index,
                ));
            }
            return Err(input_error(
                "PDF_STRUCTURE_INVALID",
                "The PDF did not pass strict validation",
                "The source may be malformed or require repair. Document Studio did not modify it.",
                OperationStage::Preflight,
                false,
                input_index,
            ));
        }
        Err(_) => return Err(process_error(OperationStage::Preflight)),
    }
    let pages = qpdf_page_count(
        runtime,
        workspace,
        &relative,
        token,
        OperationStage::Preflight,
    )?;
    if pages == 0 {
        return Err(input_error(
            "PDF_ZERO_PAGES",
            "The PDF has no pages",
            "Choose a PDF with at least one page.",
            OperationStage::Preflight,
            false,
            input_index,
        ));
    }
    Ok(PreflightResult {
        identity: source.file_identity.clone(),
        page_count: pages,
    })
}

fn verify_output(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    staging_path: &Path,
    expected_pages: u64,
    token: &CancellationToken,
) -> Result<(u64, String), OperationError> {
    if staging_path != workspace.root.join(MERGED_STAGING_RELATIVE_PATH) {
        return Err(verify_error("OUTPUT_PATH_INVALID"));
    }
    let (canonical, _) =
        canonical_regular_file(staging_path).map_err(|_| verify_error("OUTPUT_NOT_REGULAR"))?;
    let canonical_staging =
        canonical_directory(&workspace.staging).map_err(|_| verify_error("OUTPUT_PATH_INVALID"))?;
    if canonical.parent() != Some(canonical_staging.as_path())
        || canonical.file_name().and_then(|name| name.to_str()) != Some("merged.pdf")
    {
        return Err(verify_error("OUTPUT_PATH_INVALID"));
    }
    let metadata = fs::metadata(&canonical).map_err(|_| verify_error("OUTPUT_MISSING"))?;
    if metadata.len() < MIN_PDF_SIZE {
        return Err(verify_error("OUTPUT_SIZE_INVALID"));
    }
    let mut source = File::open(&canonical).map_err(|_| verify_error("OUTPUT_REOPEN_FAILED"))?;
    if !pdf_magic(&mut source).map_err(|_| verify_error("OUTPUT_REOPEN_FAILED"))? {
        return Err(verify_error("OUTPUT_NOT_PDF"));
    }
    check_cancelled(token, OperationStage::Verify)?;
    let (size, hash) = hash_file(&canonical).map_err(|_| verify_error("OUTPUT_HASH_FAILED"))?;
    if size != metadata.len() {
        return Err(verify_error("OUTPUT_SIZE_INVALID"));
    }
    let relative = Path::new(MERGED_STAGING_RELATIVE_PATH);
    let structural = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--check"),
        ],
        token,
        PREFLIGHT_PROCESS_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_structural_check_exit(structural.exit_code as i32)
        != Ok(StructuralCheckOutcome::Valid)
    {
        return Err(verify_error("OUTPUT_STRUCTURE_INVALID"));
    }
    let encryption = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        PREFLIGHT_PROCESS_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_encryption_check_exit(encryption.exit_code as i32)
        != Ok(EncryptionCheckOutcome::Unencrypted)
    {
        return Err(verify_error("OUTPUT_ENCRYPTION_INVALID"));
    }
    let pages = qpdf_page_count(runtime, workspace, relative, token, OperationStage::Verify)?;
    if pages != expected_pages {
        return Err(verify_error("OUTPUT_PAGE_COUNT_MISMATCH"));
    }
    Ok((size, hash))
}

fn qpdf_page_count(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
    stage: OperationStage,
) -> Result<u64, OperationError> {
    let execution = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--show-npages"),
        ],
        token,
        PREFLIGHT_PROCESS_TIMEOUT,
        stage,
    )?;
    if execution.exit_code != 0 {
        return Err(process_error(stage));
    }
    let output = std::str::from_utf8(&execution.stdout).map_err(|_| process_error(stage))?;
    let trimmed = output.trim();
    if trimmed.is_empty()
        || trimmed.len() > 20
        || !trimmed.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(process_error(stage));
    }
    trimmed.parse().map_err(|_| process_error(stage))
}

fn verify_qpdf_version(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let execution = run_qpdf(
        runtime,
        workspace,
        &[OsString::from("--version")],
        token,
        VERSION_TIMEOUT,
        OperationStage::Preflight,
    )?;
    if execution.exit_code != 0 || !crate::qpdf::version_output_is_expected(&execution.stdout) {
        return Err(dependency_error());
    }
    Ok(())
}

fn run_qpdf(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    arguments: &[OsString],
    token: &CancellationToken,
    timeout: Duration,
    stage: OperationStage,
) -> Result<SandboxExecution, OperationError> {
    run_qpdf_after_start_pause(
        runtime,
        workspace,
        arguments,
        token,
        timeout,
        stage,
        QpdfStartObservation::default(),
    )
}

#[derive(Default)]
struct QpdfStartObservation<'a> {
    pause: Option<Duration>,
    started: Option<&'a Arc<AtomicBool>>,
}

fn run_qpdf_after_start_pause(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    arguments: &[OsString],
    token: &CancellationToken,
    timeout: Duration,
    stage: OperationStage,
    observation: QpdfStartObservation<'_>,
) -> Result<SandboxExecution, OperationError> {
    check_cancelled(token, stage)?;
    let profile = ensure_production_profile().map_err(|_| dependency_error())?;
    let specification = SandboxLaunchSpec {
        executable: &runtime.executable,
        arguments,
        working_directory: &workspace.root,
        temporary_directory: &workspace.temporary,
    };
    let mut process = spawn_sandboxed(&profile, &specification)
        .map_err(|error| map_sandbox_error(error, stage))?;
    if observation.pause.is_some() || observation.started.is_some() {
        process
            .resume()
            .map_err(|error| map_sandbox_error(error, stage))?;
        if let Some(started) = observation.started {
            started.store(true, Ordering::Release);
        }
    }
    if let Some(pause) = observation.pause {
        cancellable_pause(token, pause, stage)?;
    }
    process
        .wait_with_cancellation(timeout, || token.is_cancelled())
        .map_err(|error| map_sandbox_error(error, stage))
}

fn map_sandbox_error(error: SandboxError, stage: OperationStage) -> OperationError {
    match error {
        SandboxError::Cancelled => cancelled_error(stage),
        SandboxError::Timeout => OperationError::safe(
            "QPDF_TIMEOUT",
            "The PDF engine exceeded its safe time limit",
            "No output was published. Try a smaller local merge.",
            stage,
            true,
        ),
        SandboxError::Windows { .. } => OperationError::safe(
            "QPDF_LAUNCH_FAILED",
            "The sandboxed PDF engine could not start",
            "No output was published. Retry after closing other local PDF jobs.",
            stage,
            true,
        ),
        SandboxError::Io(_) | SandboxError::Resume | SandboxError::Capture => OperationError::safe(
            "QPDF_SANDBOX_FAILED",
            "The sandboxed PDF engine could not finish safely",
            "No output was published. Retry the local operation.",
            stage,
            true,
        ),
        _ => dependency_error(),
    }
}

fn verify_ordinal_arguments(
    arguments: &[OsString],
    snapshots: &[OrdinalSnapshot],
) -> Result<(), OperationError> {
    if arguments
        .iter()
        .any(|argument| argument == "--deterministic-id")
    {
        return Err(process_error(OperationStage::Plan));
    }
    let files = arguments
        .iter()
        .filter_map(|argument| {
            let text = argument.to_string_lossy();
            text.strip_prefix("--file=").map(str::to_owned)
        })
        .collect::<Vec<_>>();
    let expected = snapshots
        .iter()
        .map(|snapshot| snapshot.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if files != expected {
        return Err(process_error(OperationStage::Plan));
    }
    let command_units = arguments
        .iter()
        .try_fold(0_usize, |total, argument| {
            total.checked_add(argument.encode_wide().count().saturating_add(3))
        })
        .ok_or_else(|| process_error(OperationStage::Plan))?;
    if command_units >= 32_767 {
        return Err(process_error(OperationStage::Plan));
    }
    Ok(())
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

fn cancellable_pause(
    token: &CancellationToken,
    duration: Duration,
    stage: OperationStage,
) -> Result<(), OperationError> {
    let started = Instant::now();
    while started.elapsed() < duration {
        check_cancelled(token, stage)?;
        std::thread::sleep(Duration::from_millis(10).min(duration));
    }
    Ok(())
}

fn check_cancelled(token: &CancellationToken, stage: OperationStage) -> Result<(), OperationError> {
    if token.is_cancelled() {
        return Err(cancelled_error(stage));
    }
    Ok(())
}

fn cancelled_error(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "CANCELLED",
        "PDF Merge was cancelled",
        "Temporary snapshots and unpublished output will be removed safely.",
        stage,
        false,
    )
}

fn open_source(
    path: &Path,
    stage: OperationStage,
    input_index: usize,
) -> Result<File, OperationError> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)
        .map_err(|error| {
            if matches!(error.raw_os_error(), Some(32 | 33)) {
                input_error(
                    "SOURCE_BUSY",
                    "The source PDF is busy",
                    "Close the program changing this file, then select it again.",
                    stage,
                    true,
                    input_index,
                )
            } else {
                inspect_error(input_index)
            }
        })
}

pub(crate) fn inspect_pdf_mime(path: &Path) -> Result<&'static str, std::io::Error> {
    if !has_pdf_extension(path) {
        return Ok("application/octet-stream");
    }
    let mut file = File::open(path)?;
    if pdf_magic(&mut file)? {
        Ok("application/pdf")
    } else {
        Ok("application/octet-stream")
    }
}

fn has_pdf_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn pdf_magic(source: &mut File) -> Result<bool, std::io::Error> {
    use std::io::{Seek, SeekFrom};
    source.seek(SeekFrom::Start(0))?;
    let mut header = [0_u8; 1024];
    let read = source.read(&mut header)?;
    source.seek(SeekFrom::Start(0))?;
    Ok(header[..read].windows(5).any(|window| window == b"%PDF-"))
}

fn modified_timestamp(metadata: &fs::Metadata) -> Result<String, std::io::Error> {
    let modified: DateTime<Utc> = metadata.modified()?.into();
    Ok(modified.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid_request() -> OperationError {
    OperationError::safe(
        "INVALID_OPERATION_REQUEST",
        "The PDF Merge request is not valid",
        "Choose between 2 and 128 ordered local PDFs and a Windows-safe .pdf output name.",
        OperationStage::Inspect,
        false,
    )
}

fn input_error(
    code: &str,
    title: &str,
    detail: &str,
    stage: OperationStage,
    retryable: bool,
    input_index: usize,
) -> OperationError {
    let mut error = OperationError::safe(code, title, detail, stage, retryable);
    error.input_index = Some(input_index);
    error
}

fn source_changed(index: usize, stage: OperationStage) -> OperationError {
    input_error(
        "SOURCE_CHANGED",
        "A source PDF changed",
        "Select the affected PDF again before retrying the merge.",
        stage,
        true,
        index,
    )
}

fn inspect_error(index: usize) -> OperationError {
    input_error(
        "INPUT_UNREADABLE",
        "A source PDF could not be read",
        "Check that the affected file still exists and is readable.",
        OperationStage::Inspect,
        true,
        index,
    )
}

fn path_error_for(index: usize) -> OperationError {
    input_error(
        "PATH_UNSAFE",
        "A selected path is not safe",
        "Choose a regular local PDF without links, network paths or special path syntax.",
        OperationStage::Preflight,
        false,
        index,
    )
}

fn canonical_input_error(
    error: PathPolicyError,
    index: usize,
    stage: OperationStage,
) -> OperationError {
    if matches!(
        error,
        PathPolicyError::Io(ref source) if matches!(source.raw_os_error(), Some(32 | 33))
    ) {
        input_error(
            "SOURCE_BUSY",
            "The source PDF is busy",
            "Close the program changing this file, then select it again.",
            stage,
            true,
            index,
        )
    } else {
        path_error_for(index)
    }
}

fn path_error() -> OperationError {
    OperationError::safe(
        "PATH_UNSAFE",
        "The selected path is not safe",
        "Choose regular local PDFs and a local destination without links or special path syntax.",
        OperationStage::Preflight,
        false,
    )
}

fn destination_input_error(index: usize) -> OperationError {
    input_error(
        "DESTINATION_IS_INPUT",
        "The destination is one of the source PDFs",
        "Choose a different output name or destination. Source PDFs are never overwritten.",
        OperationStage::Preflight,
        false,
        index,
    )
}

fn size_error(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "INPUT_SIZE_OVERFLOW",
        "The selected PDF set is too large to measure safely",
        "Choose a smaller set of local PDFs.",
        stage,
        false,
    )
}

fn preflight_error() -> OperationError {
    OperationError::safe(
        "PREFLIGHT_FAILED",
        "PDF Merge preflight could not finish",
        "Check local access and available disk space, then try again.",
        OperationStage::Preflight,
        true,
    )
}

fn insufficient_space(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "INSUFFICIENT_SPACE",
        "There is not enough available disk space",
        "Free local space or choose another destination, then retry.",
        stage,
        true,
    )
}

fn dependency_error() -> OperationError {
    OperationError::safe(
        "QPDF_RUNTIME_UNAVAILABLE",
        "The verified PDF engine is unavailable",
        "The bundled qpdf 12.3.2 runtime or its zero-capability sandbox did not pass verification.",
        OperationStage::Preflight,
        true,
    )
}

fn workspace_error() -> OperationError {
    OperationError::safe(
        "WORKSPACE_FAILED",
        "A private PDF workspace could not be prepared",
        "Close other instances and try the merge again.",
        OperationStage::Plan,
        true,
    )
}

fn write_error() -> OperationError {
    OperationError::safe(
        "SNAPSHOT_WRITE_FAILED",
        "A private PDF snapshot could not be written",
        "No source was changed. Check available local space and retry.",
        OperationStage::Execute,
        true,
    )
}

fn process_error(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "QPDF_PROCESS_FAILED",
        "The PDF engine could not complete the operation",
        "No output was published. Review the affected input state and retry.",
        stage,
        true,
    )
}

fn verify_error(code: &str) -> OperationError {
    OperationError::safe(
        code,
        "The merged PDF did not pass independent verification",
        "No output was published. Retry from the original source PDFs.",
        OperationStage::Verify,
        true,
    )
}

fn publication_error(error: PublicationError) -> OperationError {
    match error {
        PublicationError::Cancelled => cancelled_error(OperationStage::Publish),
        PublicationError::VerificationMismatch => verify_error("PUBLICATION_HASH_MISMATCH"),
        PublicationError::CollisionExhausted => OperationError::safe(
            "COLLISION_EXHAUSTED",
            "A safe output name could not be reserved",
            "Choose a different output name or destination and try again.",
            OperationStage::Publish,
            true,
        ),
        PublicationError::InsufficientSpace => insufficient_space(OperationStage::Publish),
        PublicationError::Path(_) => path_error(),
        PublicationError::Cleanup(_) => cleanup_error(),
        PublicationError::Io(_) => OperationError::safe(
            "PUBLICATION_FAILED",
            "The verified PDF could not be published",
            "No existing file was replaced. Check destination access and try again.",
            OperationStage::Publish,
            true,
        ),
    }
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "CLEANUP_FAILED",
        "Temporary PDF data could not be completely removed",
        "The job remains interrupted so exact owned cleanup can be retried safely.",
        OperationStage::Cleanup,
        true,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "PDF Merge metadata could not be saved",
        "The operation cannot report success until its local audit metadata is durable.",
        OperationStage::Audit,
        true,
    )
}
