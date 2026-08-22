use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use flate2::{write::ZlibEncoder, Compression};
use image::{
    DynamicImage, GenericImageView, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ};

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    JobInput, JobOutput, JobProgress, JobRecord, JobState, JobsCreateRequest, OperationError,
    OperationSpecEnvelope, OperationStage, OutputStatus, ProgressEvent, ProgressUnit,
    StoredOperationSpec, IMAGE_MAX_DIMENSION, IMAGE_MAX_PIXELS, IMAGE_TO_PDF_MAX_INPUTS,
    IMAGE_TO_PDF_MAX_TOTAL_INPUT_BYTES, IMAGE_TO_PDF_MAX_TOTAL_PIXELS, IMAGE_TO_PDF_OPERATION_ID,
    IMAGE_TO_PDF_VERSION, OPERATION_SPEC_SCHEMA_VERSION,
};
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
};
use crate::pdf_merge::{qpdf_page_count, run_qpdf, verify_qpdf_version};
use crate::process_sandbox::{authorize_qpdf_paths, ensure_production_profile};
use crate::publication::{
    hash_file, is_exact_owned_partial_path, partial_ownership_result_code,
    publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::qpdf::{
    interpret_encryption_check_exit, interpret_structural_check_exit, EncryptionCheckOutcome,
    StructuralCheckOutcome,
};
use crate::windows_security::{
    available_bytes, delete_open_file, identity_from_file, open_for_identity_and_delete,
};
use crate::workspace::JobWorkspace;

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const IMAGE_DECODE_ALLOCATION_LIMIT: u64 = 128 * 1024 * 1024;
const PDF_POINTS_PER_PIXEL: f32 = 1.0;
const QPDF_VERIFY_TIMEOUT: Duration = Duration::from_secs(60);
const STAGING_RELATIVE_PATH: &str = r"staging\image-to-pdf.pdf";

fn image_to_pdf_settings() -> serde_json::Value {
    json!({
        "alphaPolicy": "preserve-soft-mask",
        "colorProfilePolicy": "discard-profile-use-decoded-device-rgb-with-warning",
        "compression": "flate-lossless",
        "pageSizing": "one-point-per-oriented-pixel",
        "sourceOrder": "selected-order"
    })
}

#[derive(Clone)]
pub struct ImageToPdfService {
    state: AppState,
}

#[derive(Debug, Clone)]
struct InspectedImage {
    canonical: PathBuf,
    identity: String,
    display_name: String,
    source_path: String,
    size_bytes: u64,
    modified_at: String,
    mime_type: &'static str,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct PreparedImage {
    width: u32,
    height: u32,
    encoded_rgb: Vec<u8>,
    encoded_alpha: Option<Vec<u8>>,
    normalized_icc: bool,
}

impl ImageToPdfService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn create_job(&self, request: JobsCreateRequest) -> Result<JobRecord, OperationError> {
        if request.operation_id != IMAGE_TO_PDF_OPERATION_ID
            || request.input_paths.is_empty()
            || request.input_paths.len() > IMAGE_TO_PDF_MAX_INPUTS
        {
            return Err(invalid_request());
        }
        validate_output_name(&request.requested_output_name).map_err(|_| path_error())?;
        if !request
            .requested_output_name
            .to_ascii_lowercase()
            .ends_with(".pdf")
        {
            return Err(invalid_output_name());
        }
        let destination = canonical_directory(Path::new(&request.destination_directory))
            .map_err(|_| path_error())?;

        let inspected = request
            .input_paths
            .iter()
            .map(|path| inspect_image(Path::new(path), path))
            .collect::<Result<Vec<_>, _>>()?;
        enforce_collection_limits(&inspected)?;
        for input in &inspected {
            ensure_different_files(
                &input.canonical,
                &destination.join(&request.requested_output_name),
            )
            .map_err(|_| path_error())?;
        }

        let now = timestamp();
        let id = Uuid::new_v4().hyphenated().to_string();
        let inputs = inspected
            .iter()
            .enumerate()
            .map(|(ordinal, input)| JobInput {
                ordinal: ordinal as u32,
                display_name: input.display_name.clone(),
                source_path: input.source_path.clone(),
                canonical_path: input.canonical.to_string_lossy().into_owned(),
                file_identity: input.identity.clone(),
                size_bytes: input.size_bytes,
                modified_at: input.modified_at.clone(),
                mime_type: input.mime_type.to_owned(),
                sha256: None,
                password_reference: None,
            })
            .collect::<Vec<_>>();
        let job = JobRecord {
            id,
            operation_id: IMAGE_TO_PDF_OPERATION_ID.to_owned(),
            operation_version: IMAGE_TO_PDF_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: inputs.len() as u64,
                unit: ProgressUnit::Items,
            },
            destination_directory: destination.to_string_lossy().into_owned(),
            requested_output_name: request.requested_output_name.clone(),
            resolved_output_name: None,
            cancellation_requested_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
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
        let spec_envelope = OperationSpecEnvelope {
            schema_version: OPERATION_SPEC_SCHEMA_VERSION,
            operation_id: IMAGE_TO_PDF_OPERATION_ID.to_owned(),
            settings: image_to_pdf_settings(),
        };
        let canonical_json = serde_json::to_string(&spec_envelope).map_err(|_| metadata_error())?;
        let spec = StoredOperationSpec {
            sha256: sha256_hex(canonical_json.as_bytes()),
            canonical_json,
            envelope: spec_envelope,
            created_at: now,
        };
        self.state
            .database()
            .create_job_with_spec(&job, &spec)
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
        self.execute_registered(job_id, token, on_event)
    }

    pub fn execute<F>(&self, job_id: &str, on_event: F) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let token = self.state.cancellations.register(job_id);
        self.execute_registered(job_id, token, on_event)
    }

    fn execute_registered<F>(
        &self,
        job_id: &str,
        token: CancellationToken,
        mut on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let mut workspace = None;
        let result = self.execute_inner(job_id, &token, &mut workspace, &mut on_event);
        let result = match result {
            Ok(job) => Ok(job),
            Err(error) if error.code == "CANCELLED" => {
                self.finish_cancelled(job_id, workspace.as_ref(), &mut on_event)
            }
            Err(error) => {
                self.finish_failed(job_id, workspace.as_ref(), &error, &mut on_event)?;
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
            "INSPECTING_IMAGES",
            "Checking the selected local images",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Inspect)?;
        let spec = self
            .state
            .database()
            .get_operation_spec(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if inspecting.operation_id != IMAGE_TO_PDF_OPERATION_ID
            || inspecting.operation_version != IMAGE_TO_PDF_VERSION
            || spec.envelope.schema_version != OPERATION_SPEC_SCHEMA_VERSION
            || spec.envelope.operation_id != IMAGE_TO_PDF_OPERATION_ID
            || spec.envelope.settings != image_to_pdf_settings()
        {
            return Err(metadata_error());
        }

        let mut refreshed = Vec::with_capacity(inspecting.inputs.len());
        for input in &inspecting.inputs {
            check_cancelled(token, OperationStage::Inspect)?;
            let value = inspect_image(Path::new(&input.canonical_path), &input.source_path)?;
            if value.identity != input.file_identity
                || value.size_bytes != input.size_bytes
                || value.mime_type != input.mime_type
            {
                return Err(source_changed(OperationStage::Inspect));
            }
            refreshed.push(value);
        }
        enforce_collection_limits(&refreshed)?;

        let preflight = self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        emit(
            &preflight,
            OperationStage::Preflight,
            "CHECKING_IMAGE_BUDGETS",
            "Checking bounded decode, workspace, and destination budgets",
            true,
            on_event,
        );
        check_cancelled(token, OperationStage::Preflight)?;
        let destination = canonical_directory(Path::new(&preflight.destination_directory))
            .map_err(|_| path_error())?;
        let total_input_bytes = refreshed.iter().map(|input| input.size_bytes).sum::<u64>();
        let total_pixels = refreshed
            .iter()
            .map(|input| u64::from(input.width) * u64::from(input.height))
            .sum::<u64>();
        let raw_rgb_bytes = total_pixels.saturating_mul(4);
        let margin = 32 * 1024 * 1024_u64;
        let workspace_required = total_input_bytes
            .saturating_add(raw_rgb_bytes)
            .saturating_add(margin);
        let destination_required = raw_rgb_bytes.saturating_add(margin);
        if available_bytes(self.state.workspaces.root()).map_err(|_| preflight_error())?
            < workspace_required
            || available_bytes(&destination).map_err(|_| preflight_error())? < destination_required
        {
            return Err(insufficient_space());
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
            refreshed.len() as u64,
            ProgressUnit::Items,
            "IMAGE_ESTIMATE_READY",
            "The bounded image conversion budget is ready",
            true,
            on_event,
        )?;
        let workspace = self
            .state
            .workspaces
            .create_job(job_id)
            .map_err(|_| workspace_error())?;
        *workspace_slot = Some(workspace.clone());
        let profile = ensure_production_profile().map_err(|_| dependency_error())?;
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| dependency_error())?;
        verify_qpdf_version(&runtime, &workspace, token)?;
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Plan,
            0,
            refreshed.len() as u64,
            ProgressUnit::Items,
            "IMAGE_PLAN_READY",
            "The persisted image order and fixed conversion policy are ready",
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
            "READY_TO_CREATE_PDF",
            "The local image-to-PDF job is ready",
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
            "CREATING_IMAGE_PDF",
            "Creating one PDF page for each selected image",
            true,
            on_event,
        );

        let mut snapshots = Vec::with_capacity(refreshed.len());
        let mut source_hashes = Vec::with_capacity(refreshed.len());
        for (index, input) in refreshed.iter().enumerate() {
            check_cancelled(token, OperationStage::Execute)?;
            let snapshot_path = workspace.inputs.join(format!("image-{index:04}.snapshot"));
            let hash = snapshot_source(input, &snapshot_path, token)?;
            self.state
                .database()
                .update_input_hash(job_id, index as u32, &hash)
                .map_err(|_| metadata_error())?;
            snapshots.push(snapshot_path);
            source_hashes.push(hash);
        }

        let staging_path = workspace.root.join(STAGING_RELATIVE_PATH);
        let warnings = write_image_pdf(&snapshots, &staging_path, token, |completed| {
            self.progress(
                job_id,
                JobState::Running,
                OperationStage::Execute,
                completed,
                snapshots.len() as u64,
                ProgressUnit::Items,
                "WRITING_IMAGE_PAGES",
                "Writing the selected images in order",
                true,
                on_event,
            )
        })?;
        for (input_index, warning) in warnings {
            self.state
                .database()
                .record_warning(
                    job_id,
                    warning.0,
                    warning.1,
                    Some(input_index as u32),
                    Some(input_index as u32),
                )
                .map_err(|_| metadata_error())?;
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
            "VERIFYING_IMAGE_PDF",
            "Verifying PDF structure, page order, hashes, and source immutability",
            true,
            on_event,
        );
        let (verified_size, verified_hash) = verify_output(
            &runtime,
            &workspace,
            &staging_path,
            snapshots.len() as u64,
            token,
        )?;
        for (index, input) in refreshed.iter().enumerate() {
            check_cancelled(token, OperationStage::Verify)?;
            let current = inspect_image(&input.canonical, &input.source_path)?;
            if current.identity != input.identity || current.size_bytes != input.size_bytes {
                return Err(source_changed(OperationStage::Verify));
            }
            let (_, current_hash) = hash_file(&input.canonical).map_err(|_| inspect_error())?;
            if current_hash != source_hashes[index] {
                return Err(source_changed(OperationStage::Verify));
            }
        }
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

        self.publish(
            job_id,
            &verifying,
            &staging_path,
            &destination,
            &refreshed,
            verified_size,
            &verified_hash,
            token,
            on_event,
        )?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Audit,
            snapshots.len() as u64,
            snapshots.len() as u64,
            ProgressUnit::Items,
            "IMAGE_PDF_AUDIT_SAVED",
            "Verified image-to-PDF publication metadata has been saved",
            false,
            on_event,
        )?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Cleanup,
            snapshots.len() as u64,
            snapshots.len() as u64,
            ProgressUnit::Items,
            "CLEANING_IMAGE_WORKSPACE",
            "Removing private image conversion data",
            false,
            on_event,
        )?;
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
            "IMAGE_PDF_COMPLETED",
            "The verified image PDF is ready",
            false,
            on_event,
        );
        Ok(completed)
    }

    #[allow(clippy::too_many_arguments)]
    fn publish<F>(
        &self,
        job_id: &str,
        verifying: &JobRecord,
        staging_path: &Path,
        destination: &Path,
        inputs: &[InspectedImage],
        verified_size: u64,
        verified_hash: &str,
        token: &CancellationToken,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.progress(
            job_id,
            JobState::Verifying,
            OperationStage::Publish,
            0,
            verified_size,
            ProgressUnit::Bytes,
            "PREPARING_IMAGE_PDF_PUBLICATION",
            "Preparing verified no-overwrite publication",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Publish)?;

        let input_paths = inputs
            .iter()
            .map(|input| input.canonical.as_path())
            .collect::<Vec<_>>();
        let state_for_reservation = self.state.clone();
        let state_for_activation = self.state.clone();
        let activation_destination = destination.to_path_buf();
        let state_for_release = self.state.clone();
        let state_for_intent = self.state.clone();
        let commit_token = token.clone();
        let progress_token = token.clone();
        let result = publish_verified_staging_with_observer(
            PublicationContext {
                staging_path,
                input_paths: &input_paths,
                destination_directory: destination,
                requested_name: &verifying.requested_output_name,
                job_id,
            },
            || token.is_cancelled(),
            |completed, total| {
                let state = if progress_token.commit_started() {
                    JobState::Publishing
                } else {
                    JobState::Verifying
                };
                self.progress(
                    job_id,
                    state,
                    OperationStage::Publish,
                    completed,
                    total,
                    ProgressUnit::Bytes,
                    "COPYING_IMAGE_PDF_PARTIAL",
                    "Copying the verified PDF into the destination",
                    state == JobState::Verifying,
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
                let ownership_result_code = partial_ownership_result_code(
                    &activation_destination,
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
                    .activate_owned_partial(
                        job_id,
                        &partial.to_string_lossy(),
                        &ownership_result_code,
                    )
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
                if !commit_token.try_begin_publication_commit() {
                    return Err(PublicationError::Cancelled);
                }
                let resolved_name = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        PublicationError::Io(std::io::Error::other("publication name is invalid"))
                    })?;
                state_for_intent
                    .database()
                    .begin_publication(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        verified_size,
                        verified_hash,
                    )
                    .map_err(|_| {
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
            "IMAGE_PDF_PUBLICATION_COMMITTED",
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
        Ok(())
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
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        if let Err(error) = self.reconcile_temporary_artifacts(job_id, workspace) {
            let current = self.current_job(job_id)?;
            let mut database = self.state.database();
            database
                .record_error_once(job_id, &error)
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
            "IMAGE_PDF_CANCELLED",
            "Image-to-PDF conversion was cancelled and temporary data was removed",
            false,
            on_event,
        );
        Ok(cancelled)
    }

    fn finish_failed<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let current = self.current_job(job_id)?;
        let cleanup_result = self.reconcile_temporary_artifacts(job_id, workspace);
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        if let Err(cleanup) = cleanup_result {
            database
                .record_error_once(job_id, &cleanup)
                .map_err(|_| metadata_error())?;
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            return Ok(());
        }
        if current.state == JobState::Publishing {
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
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
            "IMAGE_PDF_FAILED",
            "Image-to-PDF conversion failed and temporary data was removed",
            false,
            on_event,
        );
        Ok(())
    }

    fn reconcile_temporary_artifacts(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
    ) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        let output = job.outputs.first().ok_or_else(metadata_error)?;
        if let Some(partial_path) = output.partial_path.as_deref() {
            let partial_path = Path::new(partial_path);
            let destination = Path::new(&job.destination_directory);
            if !is_exact_owned_partial_path(destination, job_id, partial_path) {
                return Err(cleanup_error());
            }
            match open_for_identity_and_delete(partial_path) {
                Ok(file) => {
                    let identity = identity_from_file(&file).map_err(|_| cleanup_error())?;
                    let ownership_result_code =
                        partial_ownership_result_code(destination, job_id, partial_path, identity)
                            .ok_or_else(cleanup_error)?;
                    if self
                        .state
                        .database()
                        .owned_partial_is_activated(
                            job_id,
                            &partial_path.to_string_lossy(),
                            &ownership_result_code,
                        )
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
                .clear_owned_partial(job_id, &partial_path.to_string_lossy())
                .map_err(|_| metadata_error())?;
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

pub(crate) fn inspect_image_mime(path: &Path) -> Result<&'static str, OperationError> {
    let file = File::open(path).map_err(|_| inspect_error())?;
    let reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|_| unsupported_image())?;
    format_mime(reader.format().ok_or_else(unsupported_image)?)
}

fn inspect_image(path: &Path, source_path: &str) -> Result<InspectedImage, OperationError> {
    let (canonical, identity) = canonical_regular_file(path).map_err(|_| path_error())?;
    let metadata = fs::metadata(&canonical).map_err(|_| inspect_error())?;
    if metadata.len() == 0 {
        return Err(unsupported_image());
    }
    let modified: DateTime<Utc> = metadata.modified().map_err(|_| inspect_error())?.into();
    let file = File::open(&canonical).map_err(|_| inspect_error())?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|_| unsupported_image())?;
    let format = reader.format().ok_or_else(unsupported_image)?;
    let mime_type = format_mime(format)?;
    reader.limits(image_limits());
    let (width, height) = reader.into_dimensions().map_err(map_image_error)?;
    validate_dimensions(width, height)?;
    let display_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(path_error)?
        .to_owned();
    Ok(InspectedImage {
        canonical,
        identity: identity.to_string(),
        display_name,
        source_path: source_path.to_owned(),
        size_bytes: metadata.len(),
        modified_at: modified.to_rfc3339_opts(SecondsFormat::Secs, true),
        mime_type,
        width,
        height,
    })
}

fn enforce_collection_limits(inputs: &[InspectedImage]) -> Result<(), OperationError> {
    let total_bytes = inputs.iter().try_fold(0_u64, |total, input| {
        total.checked_add(input.size_bytes).ok_or_else(size_limit)
    })?;
    let total_pixels = inputs.iter().try_fold(0_u64, |total, input| {
        total
            .checked_add(u64::from(input.width) * u64::from(input.height))
            .ok_or_else(size_limit)
    })?;
    if total_bytes > IMAGE_TO_PDF_MAX_TOTAL_INPUT_BYTES
        || total_pixels > IMAGE_TO_PDF_MAX_TOTAL_PIXELS
    {
        return Err(size_limit());
    }
    Ok(())
}

fn format_mime(format: ImageFormat) -> Result<&'static str, OperationError> {
    match format {
        ImageFormat::Jpeg => Ok("image/jpeg"),
        ImageFormat::Png => Ok("image/png"),
        ImageFormat::WebP => Ok("image/webp"),
        _ => Err(unsupported_image()),
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), OperationError> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > IMAGE_MAX_DIMENSION
        || height > IMAGE_MAX_DIMENSION
        || pixels > IMAGE_MAX_PIXELS
    {
        return Err(size_limit());
    }
    Ok(())
}

fn image_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_DIMENSION);
    limits.max_image_height = Some(IMAGE_MAX_DIMENSION);
    limits.max_alloc = Some(IMAGE_DECODE_ALLOCATION_LIMIT);
    limits
}

fn snapshot_source(
    input: &InspectedImage,
    snapshot_path: &Path,
    token: &CancellationToken,
) -> Result<String, OperationError> {
    let mut source = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(&input.canonical)
        .map_err(|_| inspect_error())?;
    if identity_from_file(&source)
        .map_err(|_| inspect_error())?
        .to_string()
        != input.identity
    {
        return Err(source_changed(OperationStage::Execute));
    }
    let mut snapshot = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(snapshot_path)
        .map_err(|_| workspace_error())?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        check_cancelled(token, OperationStage::Execute)?;
        let read = source.read(&mut buffer).map_err(|_| inspect_error())?;
        if read == 0 {
            break;
        }
        snapshot
            .write_all(&buffer[..read])
            .map_err(|_| workspace_error())?;
        hasher.update(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
    snapshot.sync_all().map_err(|_| workspace_error())?;
    if total != input.size_bytes {
        return Err(source_changed(OperationStage::Execute));
    }
    Ok(digest_hex(&hasher.finalize()))
}

fn prepare_image(path: &Path, token: &CancellationToken) -> Result<PreparedImage, OperationError> {
    let file = File::open(path).map_err(|_| inspect_error())?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|_| unsupported_image())?;
    let format = reader.format().ok_or_else(unsupported_image)?;
    format_mime(format)?;
    reader.limits(image_limits());
    let mut decoder = reader.into_decoder().map_err(map_image_error)?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    let orientation = decoder.orientation().map_err(map_image_error)?;
    let normalized_icc = decoder.icc_profile().map_err(map_image_error)?.is_some();
    let mut dynamic = DynamicImage::from_decoder(decoder).map_err(map_image_error)?;
    dynamic.apply_orientation(orientation);
    let (width, height) = dynamic.dimensions();
    validate_dimensions(width, height)?;

    let rgba = dynamic.to_rgba8();
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| size_limit())?;
    let mut rgb = Vec::with_capacity(pixels.saturating_mul(3));
    let mut alpha = Vec::with_capacity(pixels);
    let mut has_alpha = false;
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel.0[3]);
        has_alpha |= pixel.0[3] != u8::MAX;
    }
    let encoded_rgb = zlib(&rgb, token)?;
    let encoded_alpha = if has_alpha {
        Some(zlib(&alpha, token)?)
    } else {
        None
    };
    Ok(PreparedImage {
        width,
        height,
        encoded_rgb,
        encoded_alpha,
        normalized_icc,
    })
}

fn zlib(bytes: &[u8], token: &CancellationToken) -> Result<Vec<u8>, OperationError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    for chunk in bytes.chunks(COPY_BUFFER_SIZE) {
        check_cancelled(token, OperationStage::Execute)?;
        encoder.write_all(chunk).map_err(|_| write_error())?;
    }
    encoder.finish().map_err(|_| write_error())
}

type ImageWarning = (usize, (&'static str, &'static str));

fn write_image_pdf<F>(
    snapshots: &[PathBuf],
    output: &Path,
    token: &CancellationToken,
    mut on_page: F,
) -> Result<Vec<ImageWarning>, OperationError>
where
    F: FnMut(u64) -> Result<(), OperationError>,
{
    if snapshots.is_empty() || snapshots.len() > IMAGE_TO_PDF_MAX_INPUTS {
        return Err(invalid_request());
    }
    let catalog_id = Ref::new(1);
    let pages_id = Ref::new(2);
    let page_refs = (0..snapshots.len())
        .map(|index| Ref::new(3 + (index as i32 * 4)))
        .collect::<Vec<_>>();
    let mut pdf = Pdf::new();
    pdf.set_version(1, 4);
    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id)
        .kids(page_refs.iter().copied())
        .count(page_refs.len() as i32);
    let mut warnings = Vec::new();

    for (index, snapshot) in snapshots.iter().enumerate() {
        check_cancelled(token, OperationStage::Execute)?;
        let prepared = prepare_image(snapshot, token)?;
        if prepared.normalized_icc {
            warnings.push((
                index,
                (
                    "ICC_PROFILE_NOT_RETAINED",
                    "The embedded color profile was not retained; decoded pixel values use the fixed DeviceRGB policy.",
                ),
            ));
        }
        let page_id = page_refs[index];
        let image_id = Ref::new(page_id.get() + 1);
        let alpha_id = Ref::new(page_id.get() + 2);
        let content_id = Ref::new(page_id.get() + 3);
        let image_name = Name(b"Im1");
        let width_points = prepared.width as f32 * PDF_POINTS_PER_PIXEL;
        let height_points = prepared.height as f32 * PDF_POINTS_PER_PIXEL;

        let mut page = pdf.page(page_id);
        page.parent(pages_id);
        page.media_box(Rect::new(0.0, 0.0, width_points, height_points));
        page.contents(content_id);
        page.resources().x_objects().pair(image_name, image_id);
        page.finish();

        let mut image = pdf.image_xobject(image_id, &prepared.encoded_rgb);
        image.filter(Filter::FlateDecode);
        image.width(prepared.width as i32);
        image.height(prepared.height as i32);
        image.color_space().device_rgb();
        image.bits_per_component(8);
        if prepared.encoded_alpha.is_some() {
            image.s_mask(alpha_id);
        }
        image.finish();

        if let Some(alpha) = &prepared.encoded_alpha {
            let mut mask = pdf.image_xobject(alpha_id, alpha);
            mask.filter(Filter::FlateDecode);
            mask.width(prepared.width as i32);
            mask.height(prepared.height as i32);
            mask.color_space().device_gray();
            mask.bits_per_component(8);
            mask.finish();
        }

        let mut content = Content::new();
        content.save_state();
        content.transform([width_points, 0.0, 0.0, height_points, 0.0, 0.0]);
        content.x_object(image_name);
        content.restore_state();
        pdf.stream(content_id, &content.finish());
        on_page((index + 1) as u64)?;
    }

    check_cancelled(token, OperationStage::Execute)?;
    let bytes = pdf.finish();
    if bytes.len() > i32::MAX as usize {
        return Err(size_limit());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| write_error())?;
    for chunk in bytes.chunks(COPY_BUFFER_SIZE) {
        check_cancelled(token, OperationStage::Execute)?;
        file.write_all(chunk).map_err(|_| write_error())?;
    }
    check_cancelled(token, OperationStage::Execute)?;
    file.sync_all().map_err(|_| write_error())?;
    Ok(warnings)
}

fn verify_output(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    staging_path: &Path,
    expected_pages: u64,
    token: &CancellationToken,
) -> Result<(u64, String), OperationError> {
    check_cancelled(token, OperationStage::Verify)?;
    let metadata = fs::metadata(staging_path).map_err(|_| verify_error())?;
    if metadata.len() < 8 {
        return Err(verify_error());
    }
    let mut source = File::open(staging_path).map_err(|_| verify_error())?;
    let mut magic = [0_u8; 5];
    source.read_exact(&mut magic).map_err(|_| verify_error())?;
    if &magic != b"%PDF-" {
        return Err(verify_error());
    }
    let relative = Path::new(STAGING_RELATIVE_PATH);
    let structural = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--check"),
        ],
        token,
        QPDF_VERIFY_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_structural_check_exit(structural.exit_code as i32)
        != Ok(StructuralCheckOutcome::Valid)
    {
        return Err(verify_error());
    }
    let encryption = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        QPDF_VERIFY_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_encryption_check_exit(encryption.exit_code as i32)
        != Ok(EncryptionCheckOutcome::Unencrypted)
    {
        return Err(verify_error());
    }
    if qpdf_page_count(runtime, workspace, relative, token, OperationStage::Verify)?
        != expected_pages
    {
        return Err(verify_error());
    }
    let (size, hash) = hash_file(staging_path).map_err(|_| verify_error())?;
    if size != metadata.len() {
        return Err(verify_error());
    }
    Ok((size, hash))
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
        return Err(OperationError::safe(
            "CANCELLED",
            "Image-to-PDF conversion was cancelled",
            "No unverified output was published.",
            stage,
            false,
        ));
    }
    Ok(())
}

fn invalid_request() -> OperationError {
    OperationError::safe(
        "INVALID_INPUT_COUNT",
        "Choose between 1 and 128 images",
        "Image to PDF accepts JPEG, PNG, or WebP files in the selected order.",
        OperationStage::Inspect,
        false,
    )
}

fn invalid_output_name() -> OperationError {
    OperationError::safe(
        "INVALID_OUTPUT_NAME",
        "The output name must end in .pdf",
        "Choose a Windows-safe PDF filename without a path.",
        OperationStage::Preflight,
        false,
    )
}

fn path_error() -> OperationError {
    OperationError::safe(
        "PATH_UNSAFE",
        "The selected path is not safe",
        "Choose regular local files and an existing local destination folder.",
        OperationStage::Inspect,
        false,
    )
}

fn inspect_error() -> OperationError {
    OperationError::safe(
        "IMAGE_INSPECTION_FAILED",
        "The selected image could not be inspected",
        "Select the local image again and retry.",
        OperationStage::Inspect,
        true,
    )
}

fn unsupported_image() -> OperationError {
    OperationError::safe(
        "UNSUPPORTED_IMAGE",
        "The selected file is not a supported image",
        "Use a valid JPEG, PNG, or WebP image; file content is checked independently of its extension.",
        OperationStage::Inspect,
        false,
    )
}

fn map_image_error(error: ImageError) -> OperationError {
    if matches!(error, ImageError::Limits(_)) {
        size_limit()
    } else {
        unsupported_image()
    }
}

fn size_limit() -> OperationError {
    OperationError::safe(
        "IMAGE_RESOURCE_LIMIT",
        "The selected images exceed the safe conversion limits",
        "Each image must be at most 8192 by 8192 and 16,777,216 pixels; the selected set must stay within the bounded total budget.",
        OperationStage::Preflight,
        false,
    )
}

fn source_changed(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "SOURCE_CHANGED",
        "A source image changed",
        "Select the images again before retrying the operation.",
        stage,
        true,
    )
}

fn preflight_error() -> OperationError {
    OperationError::safe(
        "PREFLIGHT_FAILED",
        "The conversion preflight could not finish",
        "Check local storage availability and retry.",
        OperationStage::Preflight,
        true,
    )
}

fn insufficient_space() -> OperationError {
    OperationError::safe(
        "INSUFFICIENT_SPACE",
        "There is not enough available space",
        "Choose a destination with more free space or convert fewer images.",
        OperationStage::Preflight,
        true,
    )
}

fn dependency_error() -> OperationError {
    OperationError::safe(
        "QPDF_DEPENDENCY_UNAVAILABLE",
        "The PDF verifier is unavailable",
        "The accepted qpdf 12.3.2 runtime must pass its local integrity and sandbox checks.",
        OperationStage::Preflight,
        true,
    )
}

fn workspace_error() -> OperationError {
    OperationError::safe(
        "WORKSPACE_FAILED",
        "The private conversion workspace could not be prepared",
        "No output was published. Retry the local operation.",
        OperationStage::Plan,
        true,
    )
}

fn write_error() -> OperationError {
    OperationError::safe(
        "IMAGE_PDF_WRITE_FAILED",
        "The PDF could not be created",
        "No unverified output was published. Try fewer local images.",
        OperationStage::Execute,
        true,
    )
}

fn verify_error() -> OperationError {
    OperationError::safe(
        "IMAGE_PDF_VERIFY_FAILED",
        "The created PDF did not pass verification",
        "No unverified output was published.",
        OperationStage::Verify,
        false,
    )
}

fn publication_error(error: PublicationError) -> OperationError {
    match error {
        PublicationError::Cancelled => OperationError::safe(
            "CANCELLED",
            "Image-to-PDF conversion was cancelled",
            "No unverified output was published.",
            OperationStage::Publish,
            false,
        ),
        PublicationError::InsufficientSpace => insufficient_space(),
        _ => OperationError::safe(
            "IMAGE_PDF_PUBLICATION_FAILED",
            "The verified PDF could not be published",
            "No existing file was replaced. Retry with another local destination.",
            OperationStage::Publish,
            true,
        ),
    }
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "CLEANUP_FAILED",
        "Temporary image conversion data needs review",
        "The job was preserved as interrupted so cleanup can be reconciled safely.",
        OperationStage::Cleanup,
        true,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "Image-to-PDF metadata could not be read or saved",
        "The operation cannot report success until its local audit metadata is durable.",
        OperationStage::Audit,
        true,
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(&Sha256::digest(bytes))
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::{format_mime, validate_dimensions};
    use image::ImageFormat;

    #[test]
    fn codec_allow_list_is_exact() {
        assert_eq!(format_mime(ImageFormat::Jpeg).unwrap(), "image/jpeg");
        assert_eq!(format_mime(ImageFormat::Png).unwrap(), "image/png");
        assert_eq!(format_mime(ImageFormat::WebP).unwrap(), "image/webp");
        assert_eq!(
            format_mime(ImageFormat::Gif).unwrap_err().code,
            "UNSUPPORTED_IMAGE"
        );
    }

    #[test]
    fn dimensions_are_rejected_before_decode_allocation() {
        assert!(validate_dimensions(4096, 4096).is_ok());
        assert_eq!(
            validate_dimensions(8192, 8192).unwrap_err().code,
            "IMAGE_RESOURCE_LIMIT"
        );
        assert_eq!(
            validate_dimensions(8193, 1).unwrap_err().code,
            "IMAGE_RESOURCE_LIMIT"
        );
        assert_eq!(
            validate_dimensions(0, 1).unwrap_err().code,
            "IMAGE_RESOURCE_LIMIT"
        );
    }
}
