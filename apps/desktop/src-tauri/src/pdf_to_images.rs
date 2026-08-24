use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader, Limits};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    JobInput, JobOutput, JobProgress, JobRecord, JobState, OperationError, OperationSpecEnvelope,
    OperationStage, OutputStatus, PdfImageFormat, PdfPixelTransferTicket,
    PdfToImagesJobCreateRequest, PdfToImagesJobSession, ProgressEvent, ProgressUnit,
    StoredOperationSpec, CORE_PDF_MAX_PAGES, IMAGE_MAX_DIMENSION, IMAGE_MAX_PIXELS,
    OPERATION_SPEC_SCHEMA_VERSION, PDFJS_VERSION, PDF_TO_IMAGES_JPEG_QUALITY,
    PDF_TO_IMAGES_MAX_OUTPUTS, PDF_TO_IMAGES_MAX_TOTAL_PIXELS, PDF_TO_IMAGES_OPERATION_ID,
    PDF_TO_IMAGES_VERSION,
};
use crate::path_policy::validate_output_name;
use crate::pdf_merge::{qpdf_page_count, run_qpdf, verify_qpdf_version};
use crate::process_sandbox::{authorize_qpdf_paths, ensure_production_profile};
use crate::publication::{
    collision_name, hash_file, is_exact_owned_partial_path, partial_ownership_result_code,
    publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::qpdf::{
    interpret_encryption_check_exit, interpret_structural_check_exit, snapshot_relative_path,
    EncryptionCheckOutcome, StructuralCheckOutcome,
};
use crate::viewer_sessions::ViewerJobSource;
use crate::windows_security::{
    available_bytes, delete_open_file, identity_from_file, open_for_identity_and_delete,
};
use crate::workspace::JobWorkspace;

const QPDF_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_MARGIN_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelUploadMetadata {
    pub job_id: String,
    pub render_session_id: String,
    pub page_ordinal: u32,
    pub nonce: String,
    pub expected_width: u32,
    pub expected_height: u32,
}

#[derive(Clone, Default)]
pub struct PdfToImagesManager {
    jobs: Arc<Mutex<HashMap<String, Arc<Mutex<ActivePdfToImagesJob>>>>>,
}

#[derive(Debug, Clone)]
struct ActivePage {
    ticket: PdfPixelTransferTicket,
    requested_name: String,
    staging_path: PathBuf,
}

struct ActivePdfToImagesJob {
    render_session_id: String,
    source: ViewerJobSource,
    source_sha256: String,
    workspace: JobWorkspace,
    destination: PathBuf,
    format: PdfImageFormat,
    dpi: u16,
    pages: Vec<ActivePage>,
    next_ordinal: usize,
    token: CancellationToken,
}

#[derive(Clone)]
pub struct PdfToImagesService {
    state: AppState,
    hooks: PdfToImagesHooks,
}

#[derive(Clone)]
pub struct PdfToImagesHooks {
    pub before_encode: Arc<dyn Fn() + Send + Sync>,
    pub after_output_published: Arc<dyn Fn(usize) -> Result<(), OperationError> + Send + Sync>,
}

impl Default for PdfToImagesHooks {
    fn default() -> Self {
        Self {
            before_encode: Arc::new(|| {}),
            after_output_published: Arc::new(|_| Ok(())),
        }
    }
}

impl PdfToImagesService {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            hooks: PdfToImagesHooks::default(),
        }
    }

    pub fn with_hooks(state: AppState, hooks: PdfToImagesHooks) -> Self {
        Self { state, hooks }
    }

    pub fn create_job(
        &self,
        request: PdfToImagesJobCreateRequest,
    ) -> Result<PdfToImagesJobSession, OperationError> {
        validate_request(&request)?;
        let source = self
            .state
            .viewer_sessions
            .source_for_job(&request.viewer_session_id, request.viewer_generation)?;
        let destination = self
            .state
            .viewer_sessions
            .resolve_destination(&request.destination_grant_id)?;
        let output_names = reserve_output_names(
            &destination,
            &request.output_stem,
            request.format,
            request.pages.len(),
        )?;
        let now = timestamp();
        let job_id = Uuid::new_v4().hyphenated().to_string();
        let render_session_id = Uuid::new_v4().hyphenated().to_string();
        let tickets = request
            .pages
            .iter()
            .enumerate()
            .map(|(ordinal, page)| PdfPixelTransferTicket {
                page_ordinal: ordinal as u32,
                source_page_index: page.source_page_index,
                nonce: Uuid::new_v4().hyphenated().to_string(),
                expected_width: page.width,
                expected_height: page.height,
            })
            .collect::<Vec<_>>();
        let job = JobRecord {
            id: job_id.clone(),
            operation_id: PDF_TO_IMAGES_OPERATION_ID.to_owned(),
            operation_version: PDF_TO_IMAGES_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: tickets.len() as u64,
                unit: ProgressUnit::Items,
            },
            destination_directory: destination.to_string_lossy().into_owned(),
            requested_output_name: request.output_stem.clone(),
            resolved_output_name: None,
            cancellation_requested_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            finished_at: None,
            version: 0,
            inputs: vec![JobInput {
                ordinal: 0,
                display_name: source.display_name.clone(),
                source_path: source.path.to_string_lossy().into_owned(),
                canonical_path: source.path.to_string_lossy().into_owned(),
                file_identity: source.file_identity.clone(),
                size_bytes: source.size_bytes,
                modified_at: source.modified_at.clone(),
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                password_reference: None,
            }],
            outputs: output_names
                .iter()
                .enumerate()
                .map(|(ordinal, name)| JobOutput {
                    ordinal: ordinal as u32,
                    requested_name: name.clone(),
                    resolved_name: None,
                    staging_path: None,
                    partial_path: None,
                    final_path: None,
                    size_bytes: None,
                    mime_type: request.format.mime_type().to_owned(),
                    sha256: None,
                    status: OutputStatus::Planned,
                    verified_at: None,
                    published_at: None,
                })
                .collect(),
            errors: Vec::new(),
        };
        let spec_envelope = OperationSpecEnvelope {
            schema_version: OPERATION_SPEC_SCHEMA_VERSION,
            operation_id: PDF_TO_IMAGES_OPERATION_ID.to_owned(),
            settings: json!({
                "renderer": { "id": "pdfjs-dist", "version": PDFJS_VERSION },
                "encoder": { "id": "image", "version": "0.25.10" },
                "format": request.format,
                "dpi": request.dpi,
                "jpegQuality": PDF_TO_IMAGES_JPEG_QUALITY,
                "webpPolicy": "lossless",
                "background": "opaque-white",
                "canvasColorContract": "browser-pdfjs-rendered-rgb",
                "pages": request.pages,
                "outputNames": output_names,
                "aggregatePixelBudget": PDF_TO_IMAGES_MAX_TOTAL_PIXELS,
            }),
        };
        let canonical_json = serde_json::to_string(&spec_envelope).map_err(|_| metadata_error())?;
        let spec = StoredOperationSpec {
            sha256: digest_hex(Sha256::digest(canonical_json.as_bytes()).as_slice()),
            canonical_json,
            envelope: spec_envelope,
            created_at: now,
        };
        self.state
            .database()
            .create_job_with_spec(&job, &spec)
            .map_err(|_| metadata_error())?;
        let token = self.state.cancellations.register(&job_id);
        let prepared = self.prepare_job(
            &job_id,
            render_session_id.clone(),
            source,
            destination,
            request.format,
            request.dpi,
            request.source_page_count,
            tickets.clone(),
            token.clone(),
        );
        let active = match prepared {
            Ok(active) => active,
            Err(error) => {
                let _ = self.finish_unsuccessful(&job_id, &error, &mut |_| {});
                self.state.cancellations.unregister(&job_id);
                return Err(error);
            }
        };
        self.state.pdf_to_images_jobs.insert(&job_id, active)?;
        Ok(PdfToImagesJobSession {
            job: self.current_job(&job_id)?,
            render_session_id,
            pages: tickets,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_job(
        &self,
        job_id: &str,
        render_session_id: String,
        source: ViewerJobSource,
        destination: PathBuf,
        format: PdfImageFormat,
        dpi: u16,
        source_page_count: u32,
        tickets: Vec<PdfPixelTransferTicket>,
        token: CancellationToken,
    ) -> Result<ActivePdfToImagesJob, OperationError> {
        self.transition(
            job_id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )?;
        check_cancelled(&token, OperationStage::Inspect)?;
        self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        let runtime = self
            .state
            .qpdf
            .as_ref()
            .ok_or_else(dependency_error)?
            .get_or_prepare()
            .map_err(|_| dependency_error())?;
        let workspace = self
            .state
            .workspaces
            .create_job(job_id)
            .map_err(|_| workspace_error())?;
        let profile = ensure_production_profile().map_err(|_| dependency_error())?;
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| dependency_error())?;
        verify_qpdf_version(&runtime, &workspace, &token)?;
        let snapshot_relative = snapshot_relative_path(0);
        let snapshot_path = workspace.root.join(&snapshot_relative);
        let (_, source_sha256) = source.copy_snapshot(&snapshot_path, &token)?;
        self.state
            .database()
            .update_input_hash(job_id, 0, &source_sha256)
            .map_err(|_| metadata_error())?;
        verify_input_pdf(
            &runtime,
            &workspace,
            &snapshot_relative,
            u64::from(source_page_count),
            &token,
        )?;
        let required = PDF_TO_IMAGES_MAX_TOTAL_PIXELS
            .saturating_mul(4)
            .saturating_add(source.size_bytes)
            .saturating_add(OUTPUT_MARGIN_BYTES);
        if available_bytes(self.state.workspaces.root()).map_err(|_| preflight_error())? < required
            || available_bytes(&destination).map_err(|_| preflight_error())?
                < PDF_TO_IMAGES_MAX_TOTAL_PIXELS
                    .saturating_mul(4)
                    .saturating_add(OUTPUT_MARGIN_BYTES)
        {
            return Err(insufficient_space());
        }
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Estimate,
            0,
            tickets.len() as u64,
            "PDF_IMAGE_BUDGET_READY",
            "The bounded page-render work budget is ready",
            true,
            &mut |_| {},
        )?;
        self.transition(
            job_id,
            JobState::Preflight,
            JobState::Ready,
            OperationStage::Plan,
        )?;
        self.transition(
            job_id,
            JobState::Ready,
            JobState::Running,
            OperationStage::Execute,
        )?;
        let outputs = self.current_job(job_id)?.outputs;
        let pages = tickets
            .into_iter()
            .zip(outputs)
            .map(|(ticket, output)| ActivePage {
                staging_path: workspace.staging.join(format!(
                    "output-{:04}.{}",
                    ticket.page_ordinal,
                    format.extension()
                )),
                ticket,
                requested_name: output.requested_name,
            })
            .collect();
        Ok(ActivePdfToImagesJob {
            render_session_id,
            source,
            source_sha256,
            workspace,
            destination,
            format,
            dpi,
            pages,
            next_ordinal: 0,
            token,
        })
    }

    pub fn submit_page<F>(
        &self,
        metadata: PixelUploadMetadata,
        rgba: Vec<u8>,
        mut on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let active = self.state.pdf_to_images_jobs.get(&metadata.job_id)?;
        let mut active = active.lock().expect("PDF-to-images job mutex poisoned");
        let result = self.submit_page_locked(&metadata, &rgba, &mut active, &mut on_event);
        drop(active);
        match result {
            Ok(job) if job.state.is_terminal() => {
                self.state.pdf_to_images_jobs.remove(&metadata.job_id);
                self.state.cancellations.unregister(&metadata.job_id);
                Ok(job)
            }
            Ok(job) => Ok(job),
            Err(error) => {
                self.finish_unsuccessful(&metadata.job_id, &error, &mut on_event)?;
                self.state.pdf_to_images_jobs.remove(&metadata.job_id);
                self.state.cancellations.unregister(&metadata.job_id);
                Err(error)
            }
        }
    }

    fn submit_page_locked<F>(
        &self,
        metadata: &PixelUploadMetadata,
        rgba: &[u8],
        active: &mut ActivePdfToImagesJob,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_cancelled(&active.token, OperationStage::Execute)?;
        let expected_ordinal = active.next_ordinal;
        let page = active
            .pages
            .get(expected_ordinal)
            .ok_or_else(stale_pixels)?;
        if metadata.render_session_id != active.render_session_id
            || metadata.page_ordinal as usize != expected_ordinal
            || metadata.nonce != page.ticket.nonce
            || metadata.expected_width != page.ticket.expected_width
            || metadata.expected_height != page.ticket.expected_height
        {
            return Err(stale_pixels());
        }
        enforce_dimensions(metadata.expected_width, metadata.expected_height)?;
        let expected_bytes = u64::from(metadata.expected_width)
            .checked_mul(u64::from(metadata.expected_height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(payload_mismatch)?;
        if rgba.len() != expected_bytes {
            return Err(payload_mismatch());
        }
        if rgba.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            return Err(alpha_mismatch());
        }
        let mut rgb = Vec::with_capacity(expected_bytes / 4 * 3);
        for pixel in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
        check_cancelled(&active.token, OperationStage::Execute)?;
        (self.hooks.before_encode)();
        encode_pixels(
            &page.staging_path,
            &rgb,
            metadata.expected_width,
            metadata.expected_height,
            active.format,
            active.dpi,
        )?;
        check_cancelled(&active.token, OperationStage::Verify)?;
        let verified = verify_encoded_image(
            &page.staging_path,
            &rgb,
            metadata.expected_width,
            metadata.expected_height,
            active.format,
            active.dpi,
        )?;
        check_cancelled(&active.token, OperationStage::Verify)?;
        self.state
            .database()
            .set_output_staging_at(
                &metadata.job_id,
                metadata.page_ordinal,
                &page.staging_path.to_string_lossy(),
                verified.0,
                &verified.1,
                &timestamp(),
            )
            .map_err(|_| metadata_error())?;
        active.next_ordinal += 1;
        self.progress(
            &metadata.job_id,
            JobState::Running,
            OperationStage::Execute,
            active.next_ordinal as u64,
            active.pages.len() as u64,
            "PDF_IMAGE_PAGE_VERIFIED",
            "One selected page was rendered, encoded, and verified",
            true,
            on_event,
        )?;
        if active.next_ordinal < active.pages.len() {
            return self.current_job(&metadata.job_id);
        }
        self.finalize(&metadata.job_id, active, on_event)
    }

    fn finalize<F>(
        &self,
        job_id: &str,
        active: &ActivePdfToImagesJob,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_cancelled(&active.token, OperationStage::Verify)?;
        self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        verify_exact_staging_membership(active)?;
        active
            .source
            .verify_unchanged_hash(&active.source_sha256, &active.token)?;
        let verified_staging = self.verify_all_staging_hashes(job_id, active)?;
        for (ordinal, (page, (verified_size, verified_hash))) in
            active.pages.iter().zip(verified_staging).enumerate()
        {
            check_cancelled(&active.token, OperationStage::Publish)?;
            let state_for_reservation = self.state.clone();
            let state_for_activation = self.state.clone();
            let destination_for_activation = active.destination.clone();
            let state_for_release = self.state.clone();
            let state_for_intent = self.state.clone();
            let token_for_commit = active.token.clone();
            let ordinal_u32 = ordinal as u32;
            let verified_hash_for_reservation = verified_hash.clone();
            let result = publish_verified_staging_with_observer(
                PublicationContext {
                    staging_path: &page.staging_path,
                    input_paths: &[active.source.path.as_path()],
                    destination_directory: &active.destination,
                    requested_name: &page.requested_name,
                    job_id,
                },
                || active.token.is_cancelled(),
                |_, _| Ok(()),
                move |candidate, partial, resolved_name, size, sha256| {
                    if size != verified_size || sha256 != verified_hash_for_reservation {
                        return Err(PublicationError::VerificationMismatch);
                    }
                    state_for_reservation
                        .database()
                        .reserve_publication_attempt_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            &partial.to_string_lossy(),
                            size,
                            sha256,
                        )
                        .map_err(database_publication_error)
                },
                move |partial, identity| {
                    let ownership = partial_ownership_result_code(
                        &destination_for_activation,
                        job_id,
                        partial,
                        identity,
                    )
                    .ok_or_else(publication_io_error)?;
                    state_for_activation
                        .database()
                        .activate_owned_partial_at(
                            job_id,
                            ordinal_u32,
                            &partial.to_string_lossy(),
                            &ownership,
                        )
                        .map_err(database_publication_error)
                },
                move |partial| {
                    state_for_release
                        .database()
                        .clear_owned_partial_at(job_id, ordinal_u32, &partial.to_string_lossy())
                        .map_err(database_publication_error)
                },
                move |candidate| {
                    let already_started = token_for_commit.commit_started();
                    if !token_for_commit.try_begin_publication_commit() {
                        return Err(PublicationError::Cancelled);
                    }
                    let resolved_name = candidate
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(publication_io_error)?;
                    let write = if already_started {
                        state_for_intent.database().set_publication_intent_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            verified_size,
                            &verified_hash,
                        )
                    } else {
                        state_for_intent.database().begin_publication_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            verified_size,
                            &verified_hash,
                        )
                    };
                    write.map_err(database_publication_error)
                },
            )
            .map_err(|error| map_publication_error(error, ordinal))?;
            self.state
                .database()
                .set_output_published_at(
                    job_id,
                    ordinal_u32,
                    &result.resolved_name,
                    &result.final_path.to_string_lossy(),
                    result.size_bytes,
                    &result.sha256,
                    Some(&result.owned_partial_path.to_string_lossy()),
                )
                .map_err(|_| metadata_error())?;
            (self.hooks.after_output_published)(ordinal)?;
        }
        self.cleanup_workspace_and_staging(job_id)?;
        let current = self.current_job(job_id)?;
        let completed = self.transition(
            job_id,
            current.state,
            JobState::Completed,
            OperationStage::Cleanup,
        )?;
        emit(
            &completed,
            OperationStage::Cleanup,
            "PDF_TO_IMAGES_COMPLETED",
            "Every selected page is a verified published image",
            false,
            on_event,
        );
        Ok(completed)
    }

    fn verify_all_staging_hashes(
        &self,
        job_id: &str,
        active: &ActivePdfToImagesJob,
    ) -> Result<Vec<(u64, String)>, OperationError> {
        let job = self.current_job(job_id)?;
        if job.outputs.len() != active.pages.len() {
            return Err(verify_error());
        }
        let mut evidence = Vec::with_capacity(active.pages.len());
        for (page, output) in active.pages.iter().zip(&job.outputs) {
            check_cancelled(&active.token, OperationStage::Verify)?;
            let staging = page.staging_path.to_string_lossy();
            let expected_size = output.size_bytes.ok_or_else(verify_error)?;
            let expected_hash = output.sha256.as_deref().ok_or_else(verify_error)?;
            if output.status != OutputStatus::Verified
                || output.staging_path.as_deref() != Some(staging.as_ref())
            {
                return Err(verify_error());
            }
            let actual = hash_file(&page.staging_path).map_err(|_| verify_error())?;
            if actual.0 != expected_size || actual.1 != expected_hash {
                return Err(verify_error());
            }
            evidence.push(actual);
        }
        Ok(evidence)
    }

    pub fn cancel_if_idle<F>(&self, job_id: &str, mut on_event: F) -> Result<bool, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let Some(active) = self.state.pdf_to_images_jobs.find(job_id) else {
            return Ok(false);
        };
        let outcome = match active.try_lock() {
            Ok(_guard) => {
                let error = cancelled(OperationStage::Cleanup);
                self.finish_unsuccessful(job_id, &error, &mut on_event)?;
                self.state.pdf_to_images_jobs.remove(job_id);
                self.state.cancellations.unregister(job_id);
                Ok(true)
            }
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Poisoned(_)) => Err(metadata_error()),
        };
        outcome
    }

    fn finish_unsuccessful<F>(
        &self,
        job_id: &str,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let before = self.current_job(job_id)?;
        let published = before
            .outputs
            .iter()
            .filter(|output| output.status == OutputStatus::Published)
            .count();
        let cleanup = self
            .cleanup_partials(job_id)
            .and_then(|()| self.cleanup_workspace_and_staging(job_id));
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        if published > 0 {
            database
                .record_error_once(
                    job_id,
                    &partial_publication_error(published, before.outputs.len()),
                )
                .map_err(|_| metadata_error())?;
        }
        if cleanup.is_err() {
            database
                .record_error_once(job_id, &cleanup_error())
                .map_err(|_| metadata_error())?;
            let current = database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?;
            if !current.state.is_terminal() && current.state != JobState::Interrupted {
                database
                    .mark_interrupted(job_id, current.state)
                    .map_err(|_| metadata_error())?;
            }
            return Ok(());
        }
        for output in before
            .outputs
            .iter()
            .filter(|output| output.status != OutputStatus::Published)
        {
            database
                .clear_unpublished_intent_at(job_id, output.ordinal)
                .map_err(|_| metadata_error())?;
        }
        let current = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if current.state.is_terminal() {
            return Ok(());
        }
        let terminal = if error.code == "CANCELLED" && published == 0 {
            JobState::Cancelled
        } else {
            JobState::Failed
        };
        database
            .transition_job(
                job_id,
                current.state,
                current.version,
                terminal,
                Some(OperationStage::Cleanup),
            )
            .map_err(|_| metadata_error())?;
        let finished = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        drop(database);
        emit(
            &finished,
            OperationStage::Cleanup,
            if terminal == JobState::Cancelled {
                "PDF_TO_IMAGES_CANCELLED"
            } else if published > 0 {
                "PDF_TO_IMAGES_PARTIAL_PUBLICATION"
            } else {
                "PDF_TO_IMAGES_FAILED"
            },
            if published > 0 {
                "Only some verified images were published; published user files were preserved"
            } else {
                "No unverified image was published"
            },
            false,
            on_event,
        );
        Ok(())
    }

    fn cleanup_partials(&self, job_id: &str) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        let destination = Path::new(&job.destination_directory);
        for output in &job.outputs {
            let Some(partial_path) = output.partial_path.as_deref() else {
                continue;
            };
            let partial = Path::new(partial_path);
            if !is_exact_owned_partial_path(destination, job_id, partial) {
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
                        .owned_partial_is_activated_at(
                            job_id,
                            output.ordinal,
                            partial_path,
                            &ownership,
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
                .clear_owned_partial_at(job_id, output.ordinal, partial_path)
                .map_err(|_| metadata_error())?;
        }
        Ok(())
    }

    fn cleanup_workspace_and_staging(&self, job_id: &str) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        self.state
            .workspaces
            .cleanup_job(job_id)
            .map_err(|_| cleanup_error())?;
        let mut database = self.state.database();
        for output in &job.outputs {
            database
                .clear_staging_path_at(job_id, output.ordinal, output.staging_path.as_deref())
                .map_err(|_| metadata_error())?;
        }
        Ok(())
    }

    fn transition(
        &self,
        job_id: &str,
        expected: JobState,
        next: JobState,
        stage: OperationStage,
    ) -> Result<JobRecord, OperationError> {
        let current = self.current_job(job_id)?;
        if current.state != expected {
            return Err(metadata_error());
        }
        self.state
            .database()
            .transition_job(job_id, expected, current.version, next, Some(stage))
            .map_err(|_| metadata_error())?;
        self.current_job(job_id)
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
        self.state
            .database()
            .update_progress(job_id, state, stage, completed, total, ProgressUnit::Items)
            .map_err(|_| metadata_error())?;
        emit(
            &self.current_job(job_id)?,
            stage,
            code,
            message,
            cancellable,
            on_event,
        );
        Ok(())
    }

    fn current_job(&self, job_id: &str) -> Result<JobRecord, OperationError> {
        self.state
            .database()
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(job_not_found)
    }
}

impl PdfToImagesManager {
    fn insert(&self, job_id: &str, active: ActivePdfToImagesJob) -> Result<(), OperationError> {
        let mut jobs = self.jobs.lock().map_err(|_| metadata_error())?;
        if jobs.contains_key(job_id) {
            return Err(metadata_error());
        }
        jobs.insert(job_id.to_owned(), Arc::new(Mutex::new(active)));
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Arc<Mutex<ActivePdfToImagesJob>>, OperationError> {
        self.jobs
            .lock()
            .map_err(|_| metadata_error())?
            .get(job_id)
            .cloned()
            .ok_or_else(stale_pixels)
    }

    fn find(&self, job_id: &str) -> Option<Arc<Mutex<ActivePdfToImagesJob>>> {
        self.jobs.lock().ok()?.get(job_id).cloned()
    }

    fn remove(&self, job_id: &str) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(job_id);
        }
    }
}

fn validate_request(request: &PdfToImagesJobCreateRequest) -> Result<(), OperationError> {
    if Uuid::parse_str(&request.viewer_session_id).is_err()
        || Uuid::parse_str(&request.destination_grant_id).is_err()
        || request.source_page_count == 0
        || request.source_page_count > CORE_PDF_MAX_PAGES
        || request.pages.is_empty()
        || request.pages.len() > PDF_TO_IMAGES_MAX_OUTPUTS
        || !matches!(request.dpi, 72 | 150 | 300)
        || request.output_stem.is_empty()
        || request.output_stem.len() > 96
    {
        return Err(invalid_request());
    }
    validate_output_name(&format!(
        "{}.{}",
        request.output_stem,
        request.format.extension()
    ))
    .map_err(|_| invalid_output_name())?;
    let mut source_pages = HashSet::with_capacity(request.pages.len());
    let mut total_pixels = 0_u64;
    for page in &request.pages {
        if page.source_page_index >= request.source_page_count
            || !source_pages.insert(page.source_page_index)
        {
            return Err(invalid_page_plan());
        }
        enforce_dimensions(page.width, page.height)?;
        total_pixels = total_pixels
            .checked_add(u64::from(page.width) * u64::from(page.height))
            .ok_or_else(aggregate_budget_error)?;
    }
    if total_pixels > PDF_TO_IMAGES_MAX_TOTAL_PIXELS {
        return Err(aggregate_budget_error());
    }
    Ok(())
}

fn enforce_dimensions(width: u32, height: u32) -> Result<(), OperationError> {
    if width == 0 || height == 0 || width > IMAGE_MAX_DIMENSION || height > IMAGE_MAX_DIMENSION {
        return Err(dimension_error());
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(dimension_error)?;
    if pixels > IMAGE_MAX_PIXELS {
        return Err(pixel_error());
    }
    Ok(())
}

fn reserve_output_names(
    destination: &Path,
    stem: &str,
    format: PdfImageFormat,
    count: usize,
) -> Result<Vec<String>, OperationError> {
    let mut reserved = HashSet::with_capacity(count);
    let mut outputs = Vec::with_capacity(count);
    for ordinal in 0..count {
        let requested = format!("{stem}-page-{:04}.{}", ordinal + 1, format.extension());
        let resolved = (0..crate::publication::MAX_COLLISION_ATTEMPTS)
            .map(|attempt| collision_name(&requested, attempt))
            .find(|candidate| {
                !destination.join(candidate).exists() && reserved.insert(candidate.clone())
            })
            .ok_or_else(collision_error)?;
        outputs.push(resolved);
    }
    Ok(outputs)
}

fn verify_input_pdf(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    expected_pages: u64,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let encrypted = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        QPDF_PREFLIGHT_TIMEOUT,
        OperationStage::Preflight,
    )?;
    if interpret_encryption_check_exit(encrypted.exit_code as i32)
        != Ok(EncryptionCheckOutcome::Unencrypted)
    {
        return Err(encrypted_pdf());
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
        QPDF_PREFLIGHT_TIMEOUT,
        OperationStage::Preflight,
    )?;
    if interpret_structural_check_exit(structural.exit_code as i32)
        != Ok(StructuralCheckOutcome::Valid)
    {
        return Err(malformed_pdf());
    }
    if qpdf_page_count(
        runtime,
        workspace,
        relative,
        token,
        OperationStage::Preflight,
    )? != expected_pages
    {
        return Err(page_count_mismatch());
    }
    Ok(())
}

pub fn encode_and_verify_pixels(
    staging_path: &Path,
    rgb: &[u8],
    width: u32,
    height: u32,
    format: PdfImageFormat,
    dpi: u16,
) -> Result<(u64, String), OperationError> {
    encode_pixels(staging_path, rgb, width, height, format, dpi)?;
    verify_encoded_image(staging_path, rgb, width, height, format, dpi)
}

fn encode_pixels(
    staging_path: &Path,
    rgb: &[u8],
    width: u32,
    height: u32,
    format: PdfImageFormat,
    dpi: u16,
) -> Result<(), OperationError> {
    enforce_dimensions(width, height)?;
    let expected_rgb = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(3))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(payload_mismatch)?;
    if rgb.len() != expected_rgb || !matches!(dpi, 72 | 150 | 300) {
        return Err(payload_mismatch());
    }
    let mut encoded = Vec::new();
    match format {
        PdfImageFormat::Png => {
            PngEncoder::new_with_quality(&mut encoded, CompressionType::Best, FilterType::Adaptive)
                .write_image(rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(|_| encode_error())?;
            insert_png_density(&mut encoded, dpi)?;
        }
        PdfImageFormat::Jpeg => {
            let mut encoder =
                JpegEncoder::new_with_quality(&mut encoded, PDF_TO_IMAGES_JPEG_QUALITY);
            encoder.set_pixel_density(PixelDensity::dpi(dpi));
            encoder
                .write_image(rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(|_| encode_error())?;
        }
        PdfImageFormat::Webp => WebPEncoder::new_lossless(&mut encoded)
            .write_image(rgb, width, height, ExtendedColorType::Rgb8)
            .map_err(|_| encode_error())?,
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging_path)
        .map_err(|_| encode_error())?;
    file.write_all(&encoded).map_err(|_| encode_error())?;
    file.sync_all().map_err(|_| encode_error())?;
    Ok(())
}

fn verify_encoded_image(
    path: &Path,
    expected_rgb: &[u8],
    width: u32,
    height: u32,
    format: PdfImageFormat,
    dpi: u16,
) -> Result<(u64, String), OperationError> {
    let bytes = fs::read(path).map_err(|_| verify_error())?;
    if bytes.is_empty()
        || image::guess_format(&bytes).map_err(|_| verify_error())? != image_format(format)
    {
        return Err(verify_error());
    }
    if format == PdfImageFormat::Png && !png_density_matches(&bytes, dpi) {
        return Err(verify_error());
    }
    if format == PdfImageFormat::Jpeg && !jpeg_density_matches(&bytes, dpi) {
        return Err(verify_error());
    }
    drop(bytes);
    let mut reader = ImageReader::with_format(
        BufReader::new(File::open(path).map_err(|_| verify_error())?),
        image_format(format),
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_DIMENSION);
    limits.max_image_height = Some(IMAGE_MAX_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| verify_error())?;
    if decoded.width() != width || decoded.height() != height || decoded.color().has_alpha() {
        return Err(verify_error());
    }
    let actual = decoded.into_rgb8().into_raw();
    match format {
        PdfImageFormat::Png | PdfImageFormat::Webp if actual != expected_rgb => {
            return Err(visual_mismatch())
        }
        PdfImageFormat::Jpeg if mean_absolute_error(&actual, expected_rgb) > 12.0 => {
            return Err(visual_mismatch())
        }
        _ => {}
    }
    hash_file(path).map_err(|_| verify_error())
}

fn mean_absolute_error(left: &[u8], right: &[u8]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::INFINITY;
    }
    let difference = left
        .iter()
        .zip(right)
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum::<u64>();
    difference as f64 / left.len() as f64
}

fn image_format(format: PdfImageFormat) -> ImageFormat {
    match format {
        PdfImageFormat::Jpeg => ImageFormat::Jpeg,
        PdfImageFormat::Png => ImageFormat::Png,
        PdfImageFormat::Webp => ImageFormat::WebP,
    }
}

fn insert_png_density(bytes: &mut Vec<u8>, dpi: u16) -> Result<(), OperationError> {
    const SIGNATURE_AND_IHDR: usize = 33;
    if bytes.len() < SIGNATURE_AND_IHDR || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(encode_error());
    }
    let pixels_per_meter = (f64::from(dpi) / 0.0254).round() as u32;
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9_u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&pixels_per_meter.to_be_bytes());
    chunk.extend_from_slice(&pixels_per_meter.to_be_bytes());
    chunk.push(1);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    bytes.splice(SIGNATURE_AND_IHDR..SIGNATURE_AND_IHDR, chunk);
    Ok(())
}

fn png_density_matches(bytes: &[u8], dpi: u16) -> bool {
    let expected = (f64::from(dpi) / 0.0254).round() as u32;
    let mut offset = 8_usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let Some(end) = offset
            .checked_add(12)
            .and_then(|base| base.checked_add(length))
        else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        if &bytes[offset + 4..offset + 8] == b"pHYs" && length == 9 {
            let x = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            let y = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
            return x == expected && y == expected && bytes[offset + 16] == 1;
        }
        offset = end;
    }
    false
}

fn jpeg_density_matches(bytes: &[u8], dpi: u16) -> bool {
    bytes.len() >= 18
        && &bytes[..4] == b"\xff\xd8\xff\xe0"
        && &bytes[6..11] == b"JFIF\0"
        && bytes[13] == 1
        && u16::from_be_bytes([bytes[14], bytes[15]]) == dpi
        && u16::from_be_bytes([bytes[16], bytes[17]]) == dpi
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn verify_exact_staging_membership(active: &ActivePdfToImagesJob) -> Result<(), OperationError> {
    let expected = active
        .pages
        .iter()
        .map(|page| page.staging_path.clone())
        .collect::<HashSet<_>>();
    let actual = fs::read_dir(&active.workspace.staging)
        .map_err(|_| verify_error())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|_| verify_error()))
        .collect::<Result<HashSet<_>, _>>()?;
    if actual != expected {
        return Err(unexpected_output());
    }
    Ok(())
}

fn emit<F>(
    job: &JobRecord,
    stage: OperationStage,
    code: &str,
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
        message_code: code.to_owned(),
        message: message.to_owned(),
        cancellable,
    });
}

fn check_cancelled(token: &CancellationToken, stage: OperationStage) -> Result<(), OperationError> {
    if token.is_cancelled() {
        Err(cancelled(stage))
    } else {
        Ok(())
    }
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn database_publication_error(_: crate::database::DatabaseError) -> PublicationError {
    PublicationError::Io(std::io::Error::other("publication metadata update failed"))
}

fn publication_io_error() -> PublicationError {
    PublicationError::Io(std::io::Error::other("publication path identity failed"))
}

fn map_publication_error(error: PublicationError, ordinal: usize) -> OperationError {
    match error {
        PublicationError::Cancelled => cancelled(OperationStage::Publish),
        PublicationError::CollisionExhausted => collision_error(),
        PublicationError::InsufficientSpace => insufficient_space(),
        PublicationError::VerificationMismatch => verify_error(),
        _ => OperationError::safe(
            "PUBLICATION_FAILED",
            "A verified image could not be published",
            format!(
                "Output {} was not published. Any earlier published images remain in the destination.",
                ordinal + 1
            ),
            OperationStage::Publish,
            true,
        ),
    }
}

fn invalid_request() -> OperationError {
    OperationError::safe(
        "INVALID_REQUEST",
        "The PDF image request is not valid",
        "Choose 1–128 unique pages, JPEG/PNG/lossless WebP, and 72, 150, or 300 DPI.",
        OperationStage::Preflight,
        false,
    )
}
fn invalid_output_name() -> OperationError {
    OperationError::safe(
        "INVALID_OUTPUT_NAME",
        "The output stem is not valid",
        "Use a Windows-safe output stem without a path.",
        OperationStage::Plan,
        false,
    )
}
fn invalid_page_plan() -> OperationError {
    OperationError::safe(
        "INVALID_PAGE_PLAN",
        "The selected page plan is not valid",
        "Every selected source page must be unique and within the opened PDF.",
        OperationStage::Plan,
        false,
    )
}
fn dimension_error() -> OperationError {
    OperationError::safe(
        "IMAGE_DIMENSION_LIMIT",
        "A rendered page is too large",
        "Each output axis must be between 1 and 8,192 pixels.",
        OperationStage::Estimate,
        false,
    )
}
fn pixel_error() -> OperationError {
    OperationError::safe(
        "IMAGE_PIXEL_LIMIT",
        "A rendered page has too many pixels",
        "Each output must contain no more than 16,777,216 pixels.",
        OperationStage::Estimate,
        false,
    )
}
fn aggregate_budget_error() -> OperationError {
    OperationError::safe(
        "AGGREGATE_PIXEL_LIMIT",
        "The selected pages exceed the job work budget",
        "Reduce the page count or DPI until estimated work is at most 67,108,864 pixels.",
        OperationStage::Estimate,
        false,
    )
}
fn stale_pixels() -> OperationError {
    OperationError::safe(
        "PIXEL_TRANSFER_REJECTED",
        "The page pixel transfer is stale or out of order",
        "Reload the PDF and start a new conversion; replayed or wrong-page pixels were rejected.",
        OperationStage::Execute,
        false,
    )
}
fn payload_mismatch() -> OperationError {
    OperationError::safe(
        "PIXEL_PAYLOAD_MISMATCH",
        "The page pixel payload size is invalid",
        "The raw RGBA byte length must exactly equal width × height × 4.",
        OperationStage::Execute,
        false,
    )
}
fn alpha_mismatch() -> OperationError {
    OperationError::safe(
        "PIXEL_ALPHA_INVALID",
        "The rendered page is not opaque",
        "PDF-to-images v1 requires an explicit opaque-white canvas background.",
        OperationStage::Verify,
        false,
    )
}
fn encrypted_pdf() -> OperationError {
    OperationError::safe(
        "PDF_ENCRYPTED",
        "The PDF is encrypted",
        "PDF-to-images v1 accepts only unencrypted local PDFs.",
        OperationStage::Preflight,
        false,
    )
}
fn malformed_pdf() -> OperationError {
    OperationError::safe(
        "PDF_MALFORMED",
        "The PDF is malformed",
        "The bundled structural verifier rejected this local PDF.",
        OperationStage::Preflight,
        false,
    )
}
fn page_count_mismatch() -> OperationError {
    OperationError::safe(
        "SOURCE_PAGE_COUNT_MISMATCH",
        "The PDF page count changed",
        "Close the PDF and open it again before converting pages.",
        OperationStage::Preflight,
        true,
    )
}
fn encode_error() -> OperationError {
    OperationError::safe(
        "IMAGE_ENCODE_FAILED",
        "The rendered page could not be encoded",
        "No unverified image was published.",
        OperationStage::Execute,
        true,
    )
}
fn verify_error() -> OperationError {
    OperationError::safe(
        "IMAGE_VERIFY_FAILED",
        "The encoded image could not be verified",
        "Magic, format, dimensions, size, or decode evidence did not match the page plan.",
        OperationStage::Verify,
        false,
    )
}
fn visual_mismatch() -> OperationError {
    OperationError::safe("IMAGE_VISUAL_MISMATCH", "The encoded image did not match the rendered page", "Lossless outputs require exact RGB pixels; JPEG v1 requires a bounded mean pixel difference.", OperationStage::Verify, false)
}
fn unexpected_output() -> OperationError {
    OperationError::safe(
        "UNEXPECTED_STAGING_OUTPUT",
        "Unexpected staging files were found",
        "Publication stopped because private staging did not exactly match the planned outputs.",
        OperationStage::Verify,
        false,
    )
}
fn cancelled(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "CANCELLED",
        "PDF-to-images was cancelled",
        "Owned private staging was removed; already published user images were preserved.",
        stage,
        false,
    )
}
fn dependency_error() -> OperationError {
    OperationError::safe(
        "LOCAL_DEPENDENCY_UNAVAILABLE",
        "A required local verifier is unavailable",
        "PDF.js is bundled locally and qpdf 12.3.2 must pass its fixed sandbox check.",
        OperationStage::Preflight,
        true,
    )
}
fn insufficient_space() -> OperationError {
    OperationError::safe(
        "INSUFFICIENT_SPACE",
        "There is not enough local space",
        "Free space in the private workspace and destination, then retry.",
        OperationStage::Preflight,
        true,
    )
}
fn collision_error() -> OperationError {
    OperationError::safe(
        "DESTINATION_COLLISION_LIMIT",
        "Output names could not be reserved",
        "Choose another output stem or destination folder.",
        OperationStage::Plan,
        true,
    )
}
fn preflight_error() -> OperationError {
    OperationError::safe(
        "PREFLIGHT_FAILED",
        "The bounded conversion preflight failed",
        "No page rendering began.",
        OperationStage::Preflight,
        true,
    )
}
fn workspace_error() -> OperationError {
    OperationError::safe(
        "WORKSPACE_CREATE_FAILED",
        "The private workspace could not be created",
        "No page rendering began.",
        OperationStage::Plan,
        true,
    )
}
fn cleanup_error() -> OperationError {
    OperationError::safe(
        "CLEANUP_FAILED",
        "Owned temporary data could not be fully reconciled",
        "The job was preserved for safe startup recovery.",
        OperationStage::Cleanup,
        true,
    )
}
fn metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "Job metadata could not be read or saved",
        "The conversion cannot continue without durable local metadata.",
        OperationStage::Audit,
        true,
    )
}
fn job_not_found() -> OperationError {
    OperationError::safe(
        "JOB_NOT_FOUND",
        "The PDF image job is unavailable",
        "Start a new local conversion.",
        OperationStage::Recovery,
        false,
    )
}
fn partial_publication_error(published: usize, expected: usize) -> OperationError {
    OperationError::safe("PARTIAL_PUBLICATION", "Only some verified images were published", format!("{published} of {expected} verified outputs were published; published user files were preserved."), OperationStage::Publish, false)
}
