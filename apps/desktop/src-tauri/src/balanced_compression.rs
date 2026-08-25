use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, write::EncoderWriter, Engine as _};
use chrono::{SecondsFormat, Utc};
use flate2::read::ZlibDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageFormat};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app_state::{AppState, CancellationToken};
use crate::balanced_metrics::{compare_rgb8, PageQualityMetrics};
use crate::contracts::{
    BalancedCompressionAudit, BalancedCompressionJobCreateRequest, BalancedCompressionSkipCount,
    BalancedCompressionSkipReason, BalancedCompressionVisualSession, BalancedRenderPageTicket,
    BalancedRenderSide, JobInput, JobOutput, JobProgress, JobRecord, JobState, OperationError,
    OperationSpecEnvelope, OperationStage, OutputStatus, ProgressEvent, ProgressUnit,
    StoredOperationSpec, ViewerDocumentMetadata, ViewerSessionRequest,
    BALANCED_COMPRESSION_JPEG_QUALITY, BALANCED_COMPRESSION_MAX_AFFECTED_PAGES,
    BALANCED_COMPRESSION_MAX_TOTAL_PIXELS, BALANCED_COMPRESSION_OPERATION_ID,
    BALANCED_COMPRESSION_PROFILE, BALANCED_COMPRESSION_VERSION, CORE_PDF_MAX_PAGES,
    OPERATION_SPEC_SCHEMA_VERSION, PDFJS_VERSION,
};
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
};
use crate::pdf_merge::{qpdf_page_count, run_qpdf_with_capture_limit, verify_qpdf_version};
use crate::process_sandbox::{authorize_qpdf_paths, ensure_production_profile};
use crate::publication::{
    hash_file, partial_ownership_result_code, publish_verified_staging_with_observer,
    PublicationContext, PublicationError,
};
use crate::qpdf::{interpret_encryption_check_exit, EncryptionCheckOutcome, VerifiedQpdfRuntime};
use crate::windows_security::{available_bytes, identity_from_file};
use crate::workspace::JobWorkspace;

const JSON_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const PIXEL_BODY_LIMIT_BYTES: usize = 67_108_864;
const QPDF_TIMEOUT: Duration = Duration::from_secs(60);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MIN_IMAGE_AXIS: u32 = 256;
const MIN_IMAGE_PIXELS: u64 = 65_536;
const MAX_IMAGE_PIXELS: u64 = 16_777_216;
const MAX_GRAPH_DEPTH: usize = 16;
const MAX_GRAPH_OBJECTS: usize = 4_096;
const MAX_CANDIDATES: usize = 256;
const MIN_IMAGE_SAVINGS: usize = 1_024;
const MIN_DOCUMENT_SAVINGS: u64 = 65_536;
const OUTPUT_MARGIN_BYTES: u64 = 64 * 1024 * 1024;
const SOURCE_RELATIVE: &str = r"inputs\source-0000.pdf";
const CANDIDATE_RELATIVE: &str = r"staging\balanced-candidate.pdf";
const PATCH_RELATIVE: &str = r"temp\balanced-patch.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalancedPixelUploadMetadata {
    pub job_id: String,
    pub render_session_id: String,
    pub page_ordinal: u32,
    pub source_page_index: u32,
    pub nonce: String,
    pub side: BalancedRenderSide,
    pub expected_width: u32,
    pub expected_height: u32,
}

#[derive(Clone, Default)]
pub struct BalancedCompressionManager {
    jobs: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveBalancedJob>>>>>,
}

struct ActiveBalancedJob {
    render_session_id: String,
    source_path: PathBuf,
    source_identity: String,
    source_size: u64,
    source_modified_at: String,
    source_sha256: String,
    candidate_size: u64,
    candidate_sha256: String,
    structural_proof_sha256: String,
    workspace: JobWorkspace,
    destination: PathBuf,
    source_session: ViewerDocumentMetadata,
    candidate_session: ViewerDocumentMetadata,
    pages: Vec<BalancedRenderPageTicket>,
    selected_images: u32,
    skipped: BTreeMap<String, u32>,
    next_page: usize,
    expected_side: BalancedRenderSide,
    source_rgb: Option<Vec<u8>>,
    source_dimensions: Option<(u32, u32)>,
    minimum_ssim: Option<f64>,
    minimum_psnr_db: Option<f64>,
    psnr_is_infinite: bool,
    maximum_changed_pixels: u64,
    maximum_total_pixels: u64,
    comparison_pixels: u64,
    token: CancellationToken,
}

#[derive(Clone)]
pub struct BalancedCompressionService {
    state: AppState,
}

struct PreparedCandidate {
    source_path: PathBuf,
    source_identity: String,
    source_size: u64,
    source_modified_at: String,
    source_sha256: String,
    candidate_size: u64,
    candidate_sha256: String,
    structural_proof_sha256: String,
    workspace: JobWorkspace,
    destination: PathBuf,
    affected_pages: Vec<u32>,
    selected_images: u32,
    skipped: BTreeMap<String, u32>,
}

#[derive(Debug, Clone)]
struct ImageUse {
    page_indexes: BTreeSet<u32>,
    safe_edges: HashSet<String>,
}

type ImageReplacement = (Vec<u8>, JsonMap<String, JsonValue>);
type ImageUseInventory = (HashMap<String, ImageUse>, HashSet<String>, u32);

#[derive(Debug)]
enum ImageReplacementFailure {
    Skip(BalancedCompressionSkipReason),
    Abort(OperationError),
}

impl From<BalancedCompressionSkipReason> for ImageReplacementFailure {
    fn from(value: BalancedCompressionSkipReason) -> Self {
        Self::Skip(value)
    }
}

impl BalancedCompressionService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn create_job(
        &self,
        request: BalancedCompressionJobCreateRequest,
    ) -> Result<JobRecord, OperationError> {
        validate_request(&request)?;
        let source_path = Path::new(&request.input_paths[0]);
        let (canonical, expected_identity) =
            canonical_regular_file(source_path).map_err(|_| path_error())?;
        let destination = canonical_directory(Path::new(&request.destination_directory))
            .map_err(|_| path_error())?;
        ensure_different_files(
            &canonical,
            &destination.join(&request.requested_output_name),
        )
        .map_err(|_| path_error())?;
        let mut source = File::open(&canonical).map_err(|_| input_error())?;
        let identity = identity_from_file(&source).map_err(|_| input_error())?;
        let metadata = source.metadata().map_err(|_| input_error())?;
        if identity != expected_identity || !pdf_magic(&mut source)? || metadata.len() == 0 {
            return Err(input_error());
        }
        let display_name = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(path_error)?
            .to_owned();
        let now = timestamp();
        let job = JobRecord {
            id: Uuid::new_v4().hyphenated().to_string(),
            operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            operation_version: BALANCED_COMPRESSION_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: 1,
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
            completion_kind: None,
            reason: None,
            inputs: vec![JobInput {
                ordinal: 0,
                display_name,
                source_path: request.input_paths[0].clone(),
                canonical_path: canonical.to_string_lossy().into_owned(),
                file_identity: identity.to_string(),
                size_bytes: metadata.len(),
                modified_at: modified_timestamp(&metadata)?,
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                password_reference: None,
            }],
            outputs: Vec::new(),
            errors: Vec::new(),
        };
        let envelope = OperationSpecEnvelope {
            schema_version: OPERATION_SPEC_SCHEMA_VERSION,
            operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            settings: balanced_settings(),
        };
        let canonical_json = serde_json::to_string(&envelope).map_err(|_| metadata_error())?;
        let spec = StoredOperationSpec {
            envelope,
            sha256: sha256_bytes(canonical_json.as_bytes()),
            canonical_json,
            created_at: now,
        };
        self.state
            .database()
            .create_job_with_spec(&job, &spec)
            .map_err(|_| metadata_error())?;
        Ok(job)
    }

    pub fn prepare_with_registered_token<F, E>(
        &self,
        job_id: &str,
        token: CancellationToken,
        mut on_event: F,
        mut on_visual_ready: E,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
        E: FnMut(BalancedCompressionVisualSession),
    {
        let result = self.prepare_inner(job_id, token.clone(), &mut on_event);
        match result {
            Ok(PreparedOutcome::NoBenefit(job)) => {
                self.state.cancellations.unregister(job_id);
                Ok(job)
            }
            Ok(PreparedOutcome::Visual(active, session)) => {
                self.state
                    .balanced_compression_jobs
                    .insert(job_id, *active)?;
                on_visual_ready(session);
                self.current_job(job_id)
            }
            Err(error) => {
                self.finish_unsuccessful(job_id, &error, &mut on_event)?;
                self.state.cancellations.unregister(job_id);
                Err(error)
            }
        }
    }

    fn prepare_inner<F>(
        &self,
        job_id: &str,
        token: CancellationToken,
        on_event: &mut F,
    ) -> Result<PreparedOutcome, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let started = std::time::Instant::now();
        check_cancelled(&token, OperationStage::Inspect)?;
        let inspecting = self.transition(
            job_id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )?;
        emit(
            &inspecting,
            OperationStage::Inspect,
            "BALANCED_INSPECTING",
            "Checking the fixed balanced-v1 request and immutable PDF source",
            true,
            on_event,
        );
        check_cancelled(&token, OperationStage::Inspect)?;
        let spec = self
            .state
            .database()
            .get_operation_spec(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if inspecting.operation_id != BALANCED_COMPRESSION_OPERATION_ID
            || inspecting.operation_version != BALANCED_COMPRESSION_VERSION
            || spec.envelope.schema_version != OPERATION_SPEC_SCHEMA_VERSION
            || spec.envelope.operation_id != BALANCED_COMPRESSION_OPERATION_ID
            || spec.envelope.settings != balanced_settings()
        {
            return Err(metadata_error());
        }
        let input = inspecting.inputs.first().ok_or_else(metadata_error)?;
        verify_source_metadata(input)?;
        let source_path = PathBuf::from(&input.canonical_path);
        let destination = canonical_directory(Path::new(&inspecting.destination_directory))
            .map_err(|_| path_error())?;
        ensure_different_files(
            &source_path,
            &destination.join(&inspecting.requested_output_name),
        )
        .map_err(|_| path_error())?;

        let preflight = self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        emit(
            &preflight,
            OperationStage::Preflight,
            "BALANCED_PREFLIGHT",
            "Strictly checking encryption, signatures, structure, pages, and resource budgets",
            true,
            on_event,
        );
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
        let source_relative = Path::new(SOURCE_RELATIVE);
        let snapshot_path = workspace.root.join(source_relative);
        let (source_size, source_hash) =
            copy_snapshot(&source_path, &snapshot_path, input, &token, started)?;
        self.state
            .database()
            .update_input_hash(job_id, 0, &source_hash)
            .map_err(|_| metadata_error())?;
        ensure_unencrypted(&runtime, &workspace, source_relative, &token)?;
        strict_qpdf_check(&runtime, &workspace, source_relative, &token)?;
        let page_count = qpdf_page_count(
            &runtime,
            &workspace,
            source_relative,
            &token,
            OperationStage::Preflight,
        )?;
        if page_count == 0 || page_count > u64::from(CORE_PDF_MAX_PAGES) {
            return Err(page_count_error());
        }
        let source_json = qpdf_json(&runtime, &workspace, source_relative, &token)?;
        refuse_signatures(&source_json)?;
        let required = source_size
            .saturating_mul(3)
            .saturating_add(OUTPUT_MARGIN_BYTES);
        if available_bytes(self.state.workspaces.root()).map_err(|_| space_error())? < required
            || available_bytes(&destination).map_err(|_| space_error())?
                < source_size.saturating_add(OUTPUT_MARGIN_BYTES)
        {
            return Err(space_error());
        }

        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Plan,
            0,
            1,
            "BALANCED_PLAN_READY",
            "The bounded page and image resource graph is ready",
            true,
            on_event,
        )?;
        self.transition(
            job_id,
            JobState::Preflight,
            JobState::Ready,
            OperationStage::Plan,
        )?;
        let running = self.transition(
            job_id,
            JobState::Ready,
            JobState::Running,
            OperationStage::Execute,
        )?;
        emit(
            &running,
            OperationStage::Execute,
            "BALANCED_SELECTING_IMAGES",
            "Selecting safe indirect RGB8 image XObjects and encoding quality-82 candidates",
            true,
            on_event,
        );
        let mut prepared = build_candidate(
            &runtime,
            &workspace,
            &source_json,
            page_count,
            &token,
            started,
        )?;
        prepared.source_path = source_path;
        prepared.source_identity = input.file_identity.clone();
        prepared.source_size = source_size;
        prepared.source_modified_at = input.modified_at.clone();
        prepared.source_sha256 = source_hash;
        prepared.workspace = workspace;
        prepared.destination = destination;

        if prepared.selected_images == 0 {
            check_cancelled(&token, OperationStage::Verify)?;
            let verifying = self.transition(
                job_id,
                JobState::Running,
                JobState::Verifying,
                OperationStage::Verify,
            )?;
            emit(
                &verifying,
                OperationStage::Verify,
                "BALANCED_NO_SAFE_CANDIDATES",
                "No safe image replacement met the fixed profile",
                true,
                on_event,
            );
            let audit = audit_without_visual(&prepared);
            self.state
                .database()
                .record_balanced_compression_audit(job_id, &audit)
                .map_err(|_| metadata_error())?;
            self.state
                .workspaces
                .cleanup_job(job_id)
                .map_err(|_| cleanup_error())?;
            check_cancelled(&token, OperationStage::Cleanup)?;
            let current = self.current_job(job_id)?;
            self.state
                .database()
                .complete_no_benefit(
                    job_id,
                    current.version,
                    crate::contracts::JobCompletionReason::SavingsThresholdNotMet,
                    &timestamp(),
                )
                .map_err(|_| metadata_error())?;
            let completed = self.current_job(job_id)?;
            emit(
                &completed,
                OperationStage::Cleanup,
                "BALANCED_NO_BENEFIT",
                "No output was created because no safe image candidate met the fixed profile",
                false,
                on_event,
            );
            return Ok(PreparedOutcome::NoBenefit(completed));
        }

        self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        let source_session = self
            .state
            .viewer_sessions
            .open_pdf(&prepared.workspace.root.join(SOURCE_RELATIVE))?;
        let candidate_session = match self
            .state
            .viewer_sessions
            .open_pdf(&prepared.workspace.root.join(CANDIDATE_RELATIVE))
        {
            Ok(session) => session,
            Err(error) => {
                let _ = close_session(&self.state, &source_session);
                return Err(error);
            }
        };
        let render_session_id = Uuid::new_v4().hyphenated().to_string();
        let pages = prepared
            .affected_pages
            .iter()
            .enumerate()
            .map(|(ordinal, page)| BalancedRenderPageTicket {
                page_ordinal: ordinal as u32,
                source_page_index: *page,
                nonce: Uuid::new_v4().hyphenated().to_string(),
            })
            .collect::<Vec<_>>();
        let skipped_image_count = prepared.skipped.values().copied().sum();
        let session = BalancedCompressionVisualSession {
            job_id: job_id.to_owned(),
            render_session_id: render_session_id.clone(),
            source: source_session.clone(),
            candidate: candidate_session.clone(),
            pages: pages.clone(),
            selected_image_count: prepared.selected_images,
            skipped_image_count,
        };
        let active = ActiveBalancedJob {
            render_session_id,
            source_path: prepared.source_path,
            source_identity: prepared.source_identity,
            source_size: prepared.source_size,
            source_modified_at: prepared.source_modified_at,
            source_sha256: prepared.source_sha256,
            candidate_size: prepared.candidate_size,
            candidate_sha256: prepared.candidate_sha256,
            structural_proof_sha256: prepared.structural_proof_sha256,
            workspace: prepared.workspace,
            destination: prepared.destination,
            source_session,
            candidate_session,
            pages,
            selected_images: prepared.selected_images,
            skipped: prepared.skipped,
            next_page: 0,
            expected_side: BalancedRenderSide::Source,
            source_rgb: None,
            source_dimensions: None,
            minimum_ssim: None,
            minimum_psnr_db: None,
            psnr_is_infinite: true,
            maximum_changed_pixels: 0,
            maximum_total_pixels: 0,
            comparison_pixels: 0,
            token,
        };
        self.progress(
            job_id,
            JobState::Verifying,
            OperationStage::Verify,
            0,
            session.pages.len() as u64,
            "BALANCED_VISUAL_READY",
            "Every affected page is ready for sequential 144 DPI source/candidate comparison",
            true,
            on_event,
        )?;
        Ok(PreparedOutcome::Visual(Box::new(active), session))
    }

    pub fn submit_page<F>(
        &self,
        metadata: BalancedPixelUploadMetadata,
        rgba: Vec<u8>,
        mut on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        if rgba.len() > PIXEL_BODY_LIMIT_BYTES {
            return Err(pixel_error());
        }
        let active = self.state.balanced_compression_jobs.get(&metadata.job_id)?;
        let mut active = active.lock().map_err(|_| metadata_error())?;
        let result = self.submit_page_locked(&metadata, rgba, &mut active, &mut on_event);
        drop(active);
        match result {
            Ok(job) if job.state.is_terminal() => {
                self.state
                    .balanced_compression_jobs
                    .remove(&metadata.job_id);
                self.state.cancellations.unregister(&metadata.job_id);
                Ok(job)
            }
            Ok(job) => Ok(job),
            Err(error) => {
                self.finish_unsuccessful(&metadata.job_id, &error, &mut on_event)?;
                self.state
                    .balanced_compression_jobs
                    .remove(&metadata.job_id);
                self.state.cancellations.unregister(&metadata.job_id);
                Err(error)
            }
        }
    }

    fn submit_page_locked<F>(
        &self,
        metadata: &BalancedPixelUploadMetadata,
        rgba: Vec<u8>,
        active: &mut ActiveBalancedJob,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_cancelled(&active.token, OperationStage::Verify)?;
        let ticket = active
            .pages
            .get(active.next_page)
            .ok_or_else(stale_pixels)?;
        if metadata.render_session_id != active.render_session_id
            || metadata.page_ordinal as usize != active.next_page
            || metadata.source_page_index != ticket.source_page_index
            || metadata.nonce != ticket.nonce
            || metadata.side != active.expected_side
            || metadata.expected_width == 0
            || metadata.expected_height == 0
            || metadata.expected_width > 8_192
            || metadata.expected_height > 8_192
        {
            return Err(stale_pixels());
        }
        let pixels = u64::from(metadata.expected_width)
            .checked_mul(u64::from(metadata.expected_height))
            .filter(|value| *value <= MAX_IMAGE_PIXELS)
            .ok_or_else(pixel_error)?;
        let expected = pixels
            .checked_mul(4)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(pixel_error)?;
        if rgba.len() != expected || rgba.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            return Err(pixel_error());
        }
        let mut rgb = Vec::new();
        rgb.try_reserve_exact(expected / 4 * 3)
            .map_err(|_| pixel_error())?;
        for pixel in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
        match metadata.side {
            BalancedRenderSide::Source => {
                active.source_rgb = Some(rgb);
                active.source_dimensions =
                    Some((metadata.expected_width, metadata.expected_height));
                active.expected_side = BalancedRenderSide::Candidate;
                self.progress(
                    &metadata.job_id,
                    JobState::Verifying,
                    OperationStage::Verify,
                    active.next_page as u64,
                    active.pages.len() as u64,
                    "BALANCED_SOURCE_PAGE_RECEIVED",
                    "The source page is held for its immediate candidate comparison",
                    true,
                    on_event,
                )?;
                self.current_job(&metadata.job_id)
            }
            BalancedRenderSide::Candidate => {
                let source_dimensions = active.source_dimensions.take().ok_or_else(stale_pixels)?;
                if source_dimensions != (metadata.expected_width, metadata.expected_height) {
                    return Err(stale_pixels());
                }
                let source = active.source_rgb.take().ok_or_else(stale_pixels)?;
                let metrics = compare_rgb8(
                    &source,
                    &rgb,
                    metadata.expected_width,
                    metadata.expected_height,
                )?;
                drop(source);
                drop(rgb);
                if !metrics.passes() {
                    return Err(quality_error());
                }
                update_visual_audit(active, metrics)?;
                active.next_page += 1;
                active.expected_side = BalancedRenderSide::Source;
                self.progress(
                    &metadata.job_id,
                    JobState::Verifying,
                    OperationStage::Verify,
                    active.next_page as u64,
                    active.pages.len() as u64,
                    "BALANCED_PAGE_VERIFIED",
                    "One affected page passed SSIM, PSNR, changed-pixel, and opaque-alpha checks",
                    true,
                    on_event,
                )?;
                if active.next_page < active.pages.len() {
                    return self.current_job(&metadata.job_id);
                }
                self.finalize(&metadata.job_id, active, on_event)
            }
        }
    }

    fn finalize<F>(
        &self,
        job_id: &str,
        active: &mut ActiveBalancedJob,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_cancelled(&active.token, OperationStage::Verify)?;
        verify_active_source(active)?;
        let candidate_path = active.workspace.root.join(CANDIDATE_RELATIVE);
        if hash_file(&candidate_path).map_err(|_| verify_error())?
            != (active.candidate_size, active.candidate_sha256.clone())
        {
            return Err(verify_error());
        }
        close_session(&self.state, &active.source_session)?;
        close_session(&self.state, &active.candidate_session)?;
        let saved = active.source_size.saturating_sub(active.candidate_size);
        let size_gate = document_savings_gate_passes(active.source_size, active.candidate_size);
        let audit = BalancedCompressionAudit {
            profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
            source_bytes: active.source_size,
            candidate_bytes: active.candidate_size,
            saved_bytes: saved,
            saved_percent: saved as f64 * 100.0 / active.source_size as f64,
            selected_images: active.selected_images,
            skipped_images: active.skipped.values().copied().sum(),
            affected_pages: active.pages.len() as u32,
            compared_pages: active.pages.len() as u32,
            minimum_ssim: active.minimum_ssim,
            minimum_psnr_db: active.minimum_psnr_db,
            psnr_is_infinite: active.psnr_is_infinite,
            maximum_changed_pixels: active.maximum_changed_pixels,
            maximum_total_pixels: active.maximum_total_pixels,
            quality_passed: true,
            size_gate_passed: size_gate,
            structural_proof_sha256: active.structural_proof_sha256.clone(),
            skipped_reasons: skip_counts(&active.skipped),
            created_at: timestamp(),
        };
        self.state
            .database()
            .record_balanced_compression_audit(job_id, &audit)
            .map_err(|_| metadata_error())?;
        if !size_gate {
            check_cancelled(&active.token, OperationStage::Verify)?;
            self.state
                .workspaces
                .cleanup_job(job_id)
                .map_err(|_| cleanup_error())?;
            check_cancelled(&active.token, OperationStage::Cleanup)?;
            let current = self.current_job(job_id)?;
            self.state
                .database()
                .complete_no_benefit(
                    job_id,
                    current.version,
                    crate::contracts::JobCompletionReason::SavingsThresholdNotMet,
                    &timestamp(),
                )
                .map_err(|_| metadata_error())?;
            let completed = self.current_job(job_id)?;
            emit(
                &completed,
                OperationStage::Cleanup,
                "BALANCED_SAVINGS_THRESHOLD_NOT_MET",
                "No output was created because the candidate did not save both 5% and 64 KiB",
                false,
                on_event,
            );
            return Ok(completed);
        }

        let current = self.current_job(job_id)?;
        let verified_at = timestamp();
        let output = JobOutput {
            ordinal: 0,
            requested_name: current.requested_output_name.clone(),
            resolved_name: None,
            staging_path: Some(candidate_path.to_string_lossy().into_owned()),
            partial_path: None,
            final_path: None,
            size_bytes: Some(active.candidate_size),
            mime_type: "application/pdf".to_owned(),
            sha256: Some(active.candidate_sha256.clone()),
            status: OutputStatus::Verified,
            verified_at: Some(verified_at),
            published_at: None,
        };
        self.state
            .database()
            .register_verified_balanced_output(job_id, current.version, &output)
            .map_err(|_| metadata_error())?;
        check_cancelled(&active.token, OperationStage::Publish)?;
        let state_for_reservation = self.state.clone();
        let state_for_activation = self.state.clone();
        let state_for_release = self.state.clone();
        let state_for_intent = self.state.clone();
        let destination_for_activation = active.destination.clone();
        let token_for_commit = active.token.clone();
        let expected_size = active.candidate_size;
        let expected_hash = active.candidate_sha256.clone();
        let expected_hash_for_reservation = expected_hash.clone();
        let result = publish_verified_staging_with_observer(
            PublicationContext {
                staging_path: &candidate_path,
                input_paths: &[active.source_path.as_path()],
                destination_directory: &active.destination,
                requested_name: &output.requested_name,
                job_id,
            },
            || active.token.is_cancelled(),
            |completed, total| {
                self.progress(
                    job_id,
                    if active.token.commit_started() {
                        JobState::Publishing
                    } else {
                        JobState::Verifying
                    },
                    OperationStage::Publish,
                    completed,
                    total,
                    "BALANCED_COPYING_OUTPUT",
                    "Copying the verified candidate through the no-overwrite publication boundary",
                    !active.token.commit_started(),
                    on_event,
                )
                .map_err(|_| std::io::Error::other("publication progress could not be stored"))
            },
            move |candidate, partial, resolved_name, size, sha256| {
                if size != expected_size || sha256 != expected_hash_for_reservation {
                    return Err(PublicationError::VerificationMismatch);
                }
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
                    .activate_owned_partial(job_id, &partial.to_string_lossy(), &ownership)
                    .map_err(database_publication_error)
            },
            move |partial| {
                state_for_release
                    .database()
                    .clear_owned_partial(job_id, &partial.to_string_lossy())
                    .map_err(database_publication_error)
            },
            move |candidate| {
                if !token_for_commit.try_begin_publication_commit() {
                    return Err(PublicationError::Cancelled);
                }
                let resolved_name = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(publication_io_error)?;
                state_for_intent
                    .database()
                    .begin_publication(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        expected_size,
                        &expected_hash,
                    )
                    .map_err(database_publication_error)
            },
        )
        .map_err(publication_error)?;
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
        self.state
            .workspaces
            .cleanup_job(job_id)
            .map_err(|_| cleanup_error())?;
        self.state
            .database()
            .clear_staging_path(job_id, Some(&candidate_path.to_string_lossy()))
            .map_err(|_| metadata_error())?;
        let current = self.current_job(job_id)?;
        self.state
            .database()
            .complete_published(job_id, current.version, &timestamp())
            .map_err(|_| metadata_error())?;
        let completed = self.current_job(job_id)?;
        emit(
            &completed,
            OperationStage::Cleanup,
            "BALANCED_COMPLETED",
            "The verified balanced PDF was published without replacing an existing file",
            false,
            on_event,
        );
        Ok(completed)
    }

    pub fn cancel_if_idle<F>(&self, job_id: &str, mut on_event: F) -> Result<bool, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let Some(active) = self.state.balanced_compression_jobs.find(job_id) else {
            return Ok(false);
        };
        let outcome = match active.try_lock() {
            Ok(active) => {
                let _ = close_session(&self.state, &active.source_session);
                let _ = close_session(&self.state, &active.candidate_session);
                drop(active);
                self.finish_unsuccessful(job_id, &cancelled(), &mut on_event)?;
                self.state.balanced_compression_jobs.remove(job_id);
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
        let Some(current) = self
            .state
            .database()
            .get_job(job_id)
            .map_err(|_| metadata_error())?
        else {
            return Ok(());
        };
        if current.state.is_terminal() {
            return Ok(());
        }
        if let Some(active) = self.state.balanced_compression_jobs.find(job_id) {
            if let Ok(active) = active.lock() {
                let _ = close_session(&self.state, &active.source_session);
                let _ = close_session(&self.state, &active.candidate_session);
            }
        }
        let cleanup = self.state.workspaces.cleanup_job(job_id);
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        let refreshed = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if refreshed.state == JobState::Publishing || cleanup.is_err() {
            database
                .mark_interrupted(job_id, refreshed.state)
                .map_err(|_| metadata_error())?;
        } else {
            if !refreshed.outputs.is_empty() {
                database
                    .clear_unpublished_intent(job_id)
                    .map_err(|_| metadata_error())?;
            }
            let refreshed = database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?;
            database
                .transition_job(
                    job_id,
                    refreshed.state,
                    refreshed.version,
                    if error.code == "CANCELLED" {
                        JobState::Cancelled
                    } else {
                        JobState::Failed
                    },
                    Some(OperationStage::Cleanup),
                )
                .map_err(|_| metadata_error())?;
        }
        let terminal = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        drop(database);
        emit(
            &terminal,
            OperationStage::Cleanup,
            if terminal.state == JobState::Cancelled {
                "BALANCED_CANCELLED"
            } else if terminal.state == JobState::Interrupted {
                "BALANCED_INTERRUPTED"
            } else {
                "BALANCED_FAILED"
            },
            "No unverified balanced output was published",
            false,
            on_event,
        );
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
            .ok_or_else(metadata_error)
    }
}

enum PreparedOutcome {
    NoBenefit(JobRecord),
    Visual(Box<ActiveBalancedJob>, BalancedCompressionVisualSession),
}

impl BalancedCompressionManager {
    fn insert(&self, job_id: &str, active: ActiveBalancedJob) -> Result<(), OperationError> {
        let mut jobs = self.jobs.lock().map_err(|_| metadata_error())?;
        if jobs.contains_key(job_id) {
            return Err(metadata_error());
        }
        jobs.insert(job_id.to_owned(), Arc::new(Mutex::new(active)));
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Arc<Mutex<ActiveBalancedJob>>, OperationError> {
        self.jobs
            .lock()
            .map_err(|_| metadata_error())?
            .get(job_id)
            .cloned()
            .ok_or_else(stale_pixels)
    }

    fn find(&self, job_id: &str) -> Option<Arc<Mutex<ActiveBalancedJob>>> {
        self.jobs.lock().ok()?.get(job_id).cloned()
    }

    fn remove(&self, job_id: &str) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(job_id);
        }
    }
}

fn build_candidate(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    source_json: &JsonValue,
    page_count: u64,
    token: &CancellationToken,
    started: std::time::Instant,
) -> Result<PreparedCandidate, OperationError> {
    let objects = qpdf_objects(source_json)?;
    if objects.len() > MAX_GRAPH_OBJECTS {
        return Err(resource_error());
    }
    let (mut uses, unsafe_images, inline_count) = collect_safe_image_uses(source_json, objects)?;
    let global_references = count_global_references(objects);
    let mut skipped = BTreeMap::<String, u32>::new();
    add_skip(
        &mut skipped,
        BalancedCompressionSkipReason::InlineImage,
        inline_count,
    );
    for reference in unsafe_images {
        uses.remove(&reference);
        add_skip(
            &mut skipped,
            BalancedCompressionSkipReason::UnsafeResourceAncestry,
            1,
        );
    }
    if uses.len() > MAX_CANDIDATES {
        return Err(resource_error());
    }
    let mut selected_images = 0_u32;
    let mut selected_references = BTreeSet::<String>::new();
    let mut affected_pages = BTreeSet::<u32>::new();
    let patch_path = workspace.root.join(PATCH_RELATIVE);
    let mut patch_file: Option<File> = None;
    let mut first_patch_entry = true;
    for (reference, image_use) in uses.into_iter().collect::<BTreeMap<_, _>>() {
        check_budget(token, started, OperationStage::Execute)?;
        if global_references.get(&reference).copied().unwrap_or(0) != image_use.safe_edges.len() {
            add_skip(
                &mut skipped,
                BalancedCompressionSkipReason::AmbiguousSharedUse,
                1,
            );
            continue;
        }
        let Some(stream) = objects
            .get(&format!("obj:{reference}"))
            .and_then(|value| value.get("stream"))
            .and_then(JsonValue::as_object)
        else {
            add_skip(
                &mut skipped,
                BalancedCompressionSkipReason::UnsafeResourceAncestry,
                1,
            );
            continue;
        };
        let mut checkpoint = || check_budget(token, started, OperationStage::Execute);
        match replacement_for_stream(stream, &mut checkpoint) {
            Ok(Some((candidate, output_dict))) => {
                let file = match patch_file.as_mut() {
                    Some(file) => file,
                    None => {
                        let mut file = OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&patch_path)
                            .map_err(|_| workspace_error())?;
                        file.write_all(b"{\"qpdf\":[{\"jsonversion\":2},{")
                            .map_err(|_| workspace_error())?;
                        patch_file.insert(file)
                    }
                };
                write_patch_entry(
                    file,
                    &reference,
                    &candidate,
                    &output_dict,
                    first_patch_entry,
                )?;
                first_patch_entry = false;
                selected_images = selected_images.checked_add(1).ok_or_else(resource_error)?;
                selected_references.insert(reference);
                affected_pages.extend(image_use.page_indexes);
                drop(candidate);
            }
            Ok(None) => add_skip(
                &mut skipped,
                BalancedCompressionSkipReason::CandidateNotSmaller,
                1,
            ),
            Err(ImageReplacementFailure::Skip(reason)) => add_skip(&mut skipped, reason, 1),
            Err(ImageReplacementFailure::Abort(error)) => return Err(error),
        }
    }
    let affected_pages = affected_pages.into_iter().collect::<Vec<_>>();
    if affected_pages.len() > BALANCED_COMPRESSION_MAX_AFFECTED_PAGES {
        return Err(resource_error());
    }
    let source_inventory = structural_inventory(source_json, &selected_references)?;
    let source_proof_hash = sha256_bytes(source_inventory.as_bytes());
    if selected_images == 0 {
        return Ok(PreparedCandidate {
            source_path: PathBuf::new(),
            source_identity: String::new(),
            source_size: fs::metadata(workspace.root.join(SOURCE_RELATIVE))
                .map_err(|_| verify_error())?
                .len(),
            source_modified_at: String::new(),
            source_sha256: String::new(),
            candidate_size: fs::metadata(workspace.root.join(SOURCE_RELATIVE))
                .map_err(|_| verify_error())?
                .len(),
            candidate_sha256: hash_file(&workspace.root.join(SOURCE_RELATIVE))
                .map_err(|_| verify_error())?
                .1,
            structural_proof_sha256: source_proof_hash,
            workspace: workspace.clone(),
            destination: PathBuf::new(),
            affected_pages,
            selected_images,
            skipped,
        });
    }
    let mut patch_file = patch_file.ok_or_else(workspace_error)?;
    patch_file
        .write_all(b"}]}")
        .map_err(|_| workspace_error())?;
    patch_file.sync_all().map_err(|_| workspace_error())?;
    drop(patch_file);
    let args = vec![
        OsString::from(SOURCE_RELATIVE),
        OsString::from(format!("--update-from-json={PATCH_RELATIVE}")),
        OsString::from("--stream-data=preserve"),
        OsString::from("--object-streams=preserve"),
        OsString::from("--preserve-unreferenced"),
        OsString::from("--deterministic-id"),
        OsString::from(CANDIDATE_RELATIVE),
    ];
    let execution = run_qpdf_with_capture_limit(
        runtime,
        workspace,
        &args,
        token,
        QPDF_TIMEOUT,
        OperationStage::Execute,
        JSON_CAPTURE_LIMIT_BYTES,
    )?;
    if execution.exit_code != 0 {
        return Err(process_error(OperationStage::Execute));
    }
    let candidate_relative = Path::new(CANDIDATE_RELATIVE);
    strict_qpdf_check(runtime, workspace, candidate_relative, token)?;
    if qpdf_page_count(
        runtime,
        workspace,
        candidate_relative,
        token,
        OperationStage::Verify,
    )? != page_count
    {
        return Err(verify_error());
    }
    let candidate_json = qpdf_json(runtime, workspace, candidate_relative, token)?;
    let candidate_inventory = structural_inventory(&candidate_json, &selected_references)?;
    if candidate_inventory != source_inventory {
        return Err(structural_error());
    }
    let candidate_path = workspace.root.join(candidate_relative);
    let (candidate_size, candidate_sha256) =
        hash_file(&candidate_path).map_err(|_| verify_error())?;
    Ok(PreparedCandidate {
        source_path: PathBuf::new(),
        source_identity: String::new(),
        source_size: 0,
        source_modified_at: String::new(),
        source_sha256: String::new(),
        candidate_size,
        candidate_sha256,
        structural_proof_sha256: source_proof_hash,
        workspace: workspace.clone(),
        destination: PathBuf::new(),
        affected_pages,
        selected_images,
        skipped,
    })
}

fn replacement_for_stream<F>(
    stream: &JsonMap<String, JsonValue>,
    checkpoint: &mut F,
) -> Result<Option<ImageReplacement>, ImageReplacementFailure>
where
    F: FnMut() -> Result<(), OperationError>,
{
    let dict = stream
        .get("dict")
        .and_then(JsonValue::as_object)
        .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
    let width = json_u32(dict.get("/Width")).ok_or(BalancedCompressionSkipReason::NonRgb8)?;
    let height = json_u32(dict.get("/Height")).ok_or(BalancedCompressionSkipReason::NonRgb8)?;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(BalancedCompressionSkipReason::BelowMinimum)?;
    if width < MIN_IMAGE_AXIS
        || height < MIN_IMAGE_AXIS
        || !(MIN_IMAGE_PIXELS..=MAX_IMAGE_PIXELS).contains(&pixels)
    {
        return Err(BalancedCompressionSkipReason::BelowMinimum.into());
    }
    if dict.get("/BitsPerComponent").and_then(JsonValue::as_u64) != Some(8) {
        return Err(BalancedCompressionSkipReason::NonRgb8.into());
    }
    if dict.get("/ColorSpace").and_then(JsonValue::as_str) != Some("/DeviceRGB") {
        return Err(BalancedCompressionSkipReason::UnsupportedColorspace.into());
    }
    for key in ["/ImageMask", "/SMask", "/Mask", "/Matte"] {
        if dict.contains_key(key) {
            return Err(BalancedCompressionSkipReason::MaskOrTransparency.into());
        }
    }
    for key in [
        "/Alternates",
        "/OPI",
        "/OC",
        "/F",
        "/FFilter",
        "/FDecodeParms",
    ] {
        if dict.contains_key(key) {
            return Err(BalancedCompressionSkipReason::ExternalOrAlternate.into());
        }
    }
    if dict.contains_key("/DecodeParms") || dict.contains_key("/Decode") {
        return Err(BalancedCompressionSkipReason::DecodeParameters.into());
    }
    let filter = single_filter(dict.get("/Filter"))
        .ok_or(BalancedCompressionSkipReason::UnsupportedFilter)?;
    if !matches!(filter, "/DCTDecode" | "/FlateDecode") {
        return Err(BalancedCompressionSkipReason::UnsupportedFilter.into());
    }
    let data = stream
        .get("data")
        .and_then(JsonValue::as_str)
        .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
    let encoded = BASE64
        .decode(data)
        .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
    checkpoint().map_err(ImageReplacementFailure::Abort)?;
    let source_rgb = if filter == "/DCTDecode" {
        decode_rgb_jpeg(&encoded, width, height)?
    } else {
        let expected = usize::try_from(pixels.saturating_mul(3))
            .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
        let limit = u64::try_from(expected)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
        let decoder = ZlibDecoder::new(encoded.as_slice());
        let mut limited = decoder.take(limit);
        let mut decoded = Vec::new();
        decoded
            .try_reserve_exact(expected)
            .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
        limited
            .read_to_end(&mut decoded)
            .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
        if decoded.len() != expected {
            return Err(BalancedCompressionSkipReason::CandidateDecode.into());
        }
        decoded
    };
    checkpoint().map_err(ImageReplacementFailure::Abort)?;
    let mut candidate = Vec::new();
    JpegEncoder::new_with_quality(&mut candidate, BALANCED_COMPRESSION_JPEG_QUALITY)
        .encode(&source_rgb, width, height, ExtendedColorType::Rgb8)
        .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
    checkpoint().map_err(ImageReplacementFailure::Abort)?;
    if candidate.len().saturating_add(MIN_IMAGE_SAVINGS) > encoded.len() {
        return Ok(None);
    }
    let decoded_candidate = decode_rgb_jpeg(&candidate, width, height)?;
    checkpoint().map_err(ImageReplacementFailure::Abort)?;
    let metrics = compare_rgb8(&source_rgb, &decoded_candidate, width, height)
        .map_err(|_| BalancedCompressionSkipReason::CandidateQuality)?;
    checkpoint().map_err(ImageReplacementFailure::Abort)?;
    if !metrics.passes() {
        return Err(BalancedCompressionSkipReason::CandidateQuality.into());
    }
    let mut output_dict = dict.clone();
    output_dict.remove("/Length");
    output_dict.remove("/DecodeParms");
    output_dict.insert(
        "/Filter".to_owned(),
        JsonValue::String("/DCTDecode".to_owned()),
    );
    Ok(Some((candidate, output_dict)))
}

fn collect_safe_image_uses<'a>(
    root: &'a JsonValue,
    objects: &'a JsonMap<String, JsonValue>,
) -> Result<ImageUseInventory, OperationError> {
    let pages = root
        .get("pages")
        .and_then(JsonValue::as_array)
        .ok_or_else(inventory_error)?;
    let mut uses = HashMap::<String, ImageUse>::new();
    let mut unsafe_images = HashSet::<String>::new();
    let mut inline_count = 0_u32;
    let mut visited_objects = 0_usize;
    for (page_index, page) in pages.iter().enumerate() {
        let page_index = u32::try_from(page_index).map_err(|_| resource_error())?;
        for image in page
            .get("images")
            .and_then(JsonValue::as_array)
            .ok_or_else(inventory_error)?
        {
            let reference = image.get("object").and_then(JsonValue::as_str);
            let name = image.get("name").and_then(JsonValue::as_str);
            match (reference, name) {
                (Some(reference), Some(_name)) if is_reference(reference) => {
                    let entry = uses
                        .entry(reference.to_owned())
                        .or_insert_with(|| ImageUse {
                            page_indexes: BTreeSet::new(),
                            safe_edges: HashSet::new(),
                        });
                    entry.page_indexes.insert(page_index);
                }
                _ => inline_count = inline_count.saturating_add(1),
            }
        }
        let page_ref = page
            .get("object")
            .and_then(JsonValue::as_str)
            .filter(|value| is_reference(value))
            .ok_or_else(inventory_error)?;
        let page_value = object_value(objects, page_ref).ok_or_else(inventory_error)?;
        let resources = page_value.get("/Resources").ok_or_else(inventory_error)?;
        let mut stack = HashSet::new();
        walk_resources(
            resources,
            objects,
            page_ref,
            page_index,
            0,
            false,
            &mut stack,
            &mut visited_objects,
            &mut uses,
            &mut unsafe_images,
        )?;
    }
    uses.retain(|_, value| !value.safe_edges.is_empty());
    Ok((uses, unsafe_images, inline_count))
}

#[allow(clippy::too_many_arguments)]
fn walk_resources(
    resources: &JsonValue,
    objects: &JsonMap<String, JsonValue>,
    container: &str,
    page_index: u32,
    depth: usize,
    unsafe_ancestry: bool,
    stack: &mut HashSet<String>,
    visited_objects: &mut usize,
    uses: &mut HashMap<String, ImageUse>,
    unsafe_images: &mut HashSet<String>,
) -> Result<(), OperationError> {
    if depth > MAX_GRAPH_DEPTH || *visited_objects > MAX_GRAPH_OBJECTS {
        return Err(resource_error());
    }
    let resources = resolve_dictionary(resources, objects).ok_or_else(inventory_error)?;
    let Some(xobjects_value) = resources.get("/XObject") else {
        return Ok(());
    };
    let xobjects = resolve_dictionary(xobjects_value, objects).ok_or_else(inventory_error)?;
    for (name, value) in xobjects {
        let Some(reference) = value.as_str().filter(|value| is_reference(value)) else {
            continue;
        };
        *visited_objects = visited_objects.checked_add(1).ok_or_else(resource_error)?;
        if *visited_objects > MAX_GRAPH_OBJECTS {
            return Err(resource_error());
        }
        let Some(stream) = object_stream(objects, reference) else {
            continue;
        };
        let dict = stream
            .get("dict")
            .and_then(JsonValue::as_object)
            .ok_or_else(inventory_error)?;
        match dict.get("/Subtype").and_then(JsonValue::as_str) {
            Some("/Image") => {
                if let Some(image_use) = uses.get_mut(reference) {
                    image_use.page_indexes.insert(page_index);
                    let edge = format!("{container}|{name}|{reference}");
                    image_use.safe_edges.insert(edge);
                    if unsafe_ancestry {
                        unsafe_images.insert(reference.to_owned());
                    }
                }
            }
            Some("/Form") => {
                let form_unsafe = unsafe_ancestry || form_has_transparency(dict);
                if !stack.insert(reference.to_owned()) {
                    return Err(resource_error());
                }
                if let Some(nested) = dict.get("/Resources") {
                    walk_resources(
                        nested,
                        objects,
                        reference,
                        page_index,
                        depth + 1,
                        form_unsafe,
                        stack,
                        visited_objects,
                        uses,
                        unsafe_images,
                    )?;
                }
                stack.remove(reference);
            }
            _ => {}
        }
    }
    Ok(())
}

fn structural_inventory(
    root: &JsonValue,
    selected_references: &BTreeSet<String>,
) -> Result<String, OperationError> {
    let objects = qpdf_objects(root)?;
    let pages = root
        .get("pages")
        .and_then(JsonValue::as_array)
        .ok_or_else(inventory_error)?;
    let mut page_inventory = Vec::new();
    for page in pages {
        let images = page
            .get("images")
            .and_then(JsonValue::as_array)
            .ok_or_else(inventory_error)?;
        let mut image_inventory = Vec::new();
        for image in images {
            let name = image
                .get("name")
                .and_then(JsonValue::as_str)
                .ok_or_else(inventory_error)?;
            let selected = image
                .get("object")
                .and_then(JsonValue::as_str)
                .is_some_and(|reference| selected_references.contains(reference));
            image_inventory.push(json!({
                "name": name,
                "object": image.get("object"),
                "width": image.get("width"),
                "height": image.get("height"),
                "bits": image.get("bitspercomponent"),
                "colorspace": image.get("colorspace"),
                "filter": if selected { json!("@selected") } else { image.get("filter").cloned().unwrap_or(JsonValue::Null) },
                "decodeparms": if selected { JsonValue::Null } else { image.get("decodeparms").cloned().unwrap_or(JsonValue::Null) },
            }));
        }
        page_inventory.push(json!({
            "object": page.get("object"),
            "images": image_inventory,
            "outlines": normalize_value(page.get("outlines").unwrap_or(&JsonValue::Null)),
        }));
    }
    let mut object_inventory = BTreeMap::<String, JsonValue>::new();
    for (key, object) in objects {
        if let Some(stream) = object.get("stream").and_then(JsonValue::as_object) {
            let reference = key.strip_prefix("obj:").unwrap_or(key);
            let selected = selected_references.contains(reference);
            let mut dict = stream.get("dict").cloned().ok_or_else(inventory_error)?;
            strip_length(&mut dict);
            if selected {
                if let Some(dict) = dict.as_object_mut() {
                    dict.insert("/Filter".to_owned(), json!("@selected"));
                    dict.remove("/DecodeParms");
                }
            }
            let data = stream
                .get("data")
                .and_then(JsonValue::as_str)
                .ok_or_else(inventory_error)?;
            object_inventory.insert(
                key.clone(),
                json!({
                    "kind": "stream",
                    "dict": normalize_value(&dict),
                    "sha256": if selected { "@selected".to_owned() } else { sha256_bytes(&BASE64.decode(data).map_err(|_| inventory_error())?) },
                }),
            );
        } else if let Some(value) = object.get("value") {
            let mut value = value.clone();
            if key == "trailer" {
                if let Some(trailer) = value.as_object_mut() {
                    // qpdf necessarily creates or refreshes the second trailer ID when it
                    // writes a changed document. The deterministic-id flag makes that
                    // boilerplate reproducible; it is not semantic document metadata.
                    trailer.remove("/ID");
                }
            }
            object_inventory.insert(
                key.clone(),
                json!({
                    "kind": "value",
                    "value": normalize_value(&value),
                }),
            );
        } else {
            return Err(inventory_error());
        }
    }
    serde_json::to_string(&json!({
        "pages": page_inventory,
        "acroform": normalize_value(root.get("acroform").unwrap_or(&JsonValue::Null)),
        "attachments": normalize_value(root.get("attachments").unwrap_or(&JsonValue::Null)),
        "outlines": normalize_value(root.get("outlines").unwrap_or(&JsonValue::Null)),
        "pagelabels": normalize_value(root.get("pagelabels").unwrap_or(&JsonValue::Null)),
        "objects": object_inventory,
    }))
    .map_err(|_| inventory_error())
}

fn normalize_value(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(object) => {
            let mut normalized = JsonMap::new();
            for (key, value) in object {
                if key == "/Length" {
                    continue;
                }
                normalized.insert(key.clone(), normalize_value(value));
            }
            JsonValue::Object(normalized)
        }
        JsonValue::Array(values) => JsonValue::Array(values.iter().map(normalize_value).collect()),
        _ => value.clone(),
    }
}

fn qpdf_json(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
) -> Result<JsonValue, OperationError> {
    let execution = run_qpdf_with_capture_limit(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--json=2"),
            OsString::from("--json-stream-data=inline"),
        ],
        token,
        QPDF_TIMEOUT,
        OperationStage::Verify,
        JSON_CAPTURE_LIMIT_BYTES,
    )?;
    if execution.exit_code != 0 {
        return Err(inventory_error());
    }
    serde_json::from_slice(&execution.stdout).map_err(|_| inventory_error())
}

fn strict_qpdf_check(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let execution = run_qpdf_with_capture_limit(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--check"),
        ],
        token,
        QPDF_TIMEOUT,
        OperationStage::Preflight,
        4 * 1024 * 1024,
    )?;
    if execution.exit_code != 0 {
        return Err(structural_error());
    }
    Ok(())
}

fn ensure_unencrypted(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let execution = run_qpdf_with_capture_limit(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        QPDF_TIMEOUT,
        OperationStage::Preflight,
        1024,
    )?;
    match interpret_encryption_check_exit(execution.exit_code as i32) {
        Ok(EncryptionCheckOutcome::Unencrypted) => Ok(()),
        Ok(EncryptionCheckOutcome::Encrypted) => Err(encryption_error()),
        Err(_) => Err(process_error(OperationStage::Preflight)),
    }
}

fn refuse_signatures(root: &JsonValue) -> Result<(), OperationError> {
    fn contains_signature(value: &JsonValue) -> bool {
        match value {
            JsonValue::Object(object) => object.iter().any(|(key, value)| {
                matches!(
                    key.as_str(),
                    "/ByteRange" | "/Sig" | "/DocMDP" | "/FieldMDP" | "/Perms"
                ) || value.as_str().is_some_and(|value| value == "/Sig")
                    || contains_signature(value)
            }),
            JsonValue::Array(values) => values.iter().any(contains_signature),
            _ => false,
        }
    }
    if contains_signature(root) {
        Err(signature_error())
    } else {
        Ok(())
    }
}

fn qpdf_objects(root: &JsonValue) -> Result<&JsonMap<String, JsonValue>, OperationError> {
    root.get("qpdf")
        .and_then(JsonValue::as_array)
        .and_then(|values| values.get(1))
        .and_then(JsonValue::as_object)
        .ok_or_else(inventory_error)
}

fn object_value<'a>(
    objects: &'a JsonMap<String, JsonValue>,
    reference: &str,
) -> Option<&'a JsonMap<String, JsonValue>> {
    objects
        .get(&format!("obj:{reference}"))?
        .get("value")?
        .as_object()
}

fn object_stream<'a>(
    objects: &'a JsonMap<String, JsonValue>,
    reference: &str,
) -> Option<&'a JsonMap<String, JsonValue>> {
    objects
        .get(&format!("obj:{reference}"))?
        .get("stream")?
        .as_object()
}

fn resolve_dictionary<'a>(
    value: &'a JsonValue,
    objects: &'a JsonMap<String, JsonValue>,
) -> Option<&'a JsonMap<String, JsonValue>> {
    if let Some(object) = value.as_object() {
        return Some(object);
    }
    object_value(objects, value.as_str()?)
}

fn count_global_references(objects: &JsonMap<String, JsonValue>) -> HashMap<String, usize> {
    fn walk(value: &JsonValue, counts: &mut HashMap<String, usize>) {
        match value {
            JsonValue::Object(object) => {
                for value in object.values() {
                    walk(value, counts);
                }
            }
            JsonValue::Array(values) => {
                for value in values {
                    walk(value, counts);
                }
            }
            JsonValue::String(value) if is_reference(value) => {
                *counts.entry(value.clone()).or_default() += 1;
            }
            _ => {}
        }
    }
    let mut counts = HashMap::new();
    for value in objects.values() {
        walk(value, &mut counts);
    }
    counts
}

fn form_has_transparency(dict: &JsonMap<String, JsonValue>) -> bool {
    dict.contains_key("/Group")
        || dict.contains_key("/SMask")
        || dict.contains_key("/Mask")
        || dict.contains_key("/Alternates")
        || dict.contains_key("/OPI")
        || dict.contains_key("/OC")
}

fn single_filter(value: Option<&JsonValue>) -> Option<&str> {
    match value? {
        JsonValue::String(value) => Some(value),
        JsonValue::Array(values) if values.len() == 1 => values[0].as_str(),
        _ => None,
    }
}

fn json_u32(value: Option<&JsonValue>) -> Option<u32> {
    u32::try_from(value?.as_u64()?).ok()
}

fn is_reference(value: &str) -> bool {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    parts.len() == 3
        && parts[0].parse::<u32>().is_ok()
        && parts[1].parse::<u16>().is_ok()
        && parts[2] == "R"
}

fn decode_rgb_jpeg(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, BalancedCompressionSkipReason> {
    verify_rgb8_jpeg_header(bytes, width, height)?;
    let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
        .map_err(|_| BalancedCompressionSkipReason::CandidateDecode)?;
    match decoded {
        image::DynamicImage::ImageRgb8(image)
            if image.width() == width && image.height() == height =>
        {
            Ok(image.into_raw())
        }
        image::DynamicImage::ImageLuma8(_) | image::DynamicImage::ImageLumaA8(_) => {
            Err(BalancedCompressionSkipReason::NonRgb8)
        }
        _ => Err(BalancedCompressionSkipReason::CandidateDecode),
    }
}

fn verify_rgb8_jpeg_header(
    bytes: &[u8],
    expected_width: u32,
    expected_height: u32,
) -> Result<(), BalancedCompressionSkipReason> {
    if bytes.len() < 4 || bytes[..2] != [0xff, 0xd8] {
        return Err(BalancedCompressionSkipReason::CandidateDecode);
    }
    let mut cursor = 2_usize;
    while cursor < bytes.len() {
        if bytes[cursor] != 0xff {
            return Err(BalancedCompressionSkipReason::CandidateDecode);
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        let marker = *bytes
            .get(cursor)
            .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
        cursor += 1;
        if marker == 0x00 {
            return Err(BalancedCompressionSkipReason::CandidateDecode);
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            return Err(BalancedCompressionSkipReason::CandidateDecode);
        }
        let length_bytes = bytes
            .get(cursor..cursor.saturating_add(2))
            .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
        let segment_length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
        if segment_length < 2 {
            return Err(BalancedCompressionSkipReason::CandidateDecode);
        }
        let payload_start = cursor + 2;
        let segment_end = cursor
            .checked_add(segment_length)
            .filter(|end| *end <= bytes.len())
            .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
        let is_start_of_frame = matches!(
            marker,
            0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
        );
        if is_start_of_frame {
            let header = bytes
                .get(payload_start..segment_end)
                .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
            if header.len() < 6 {
                return Err(BalancedCompressionSkipReason::CandidateDecode);
            }
            let precision = header[0];
            let height = u32::from(u16::from_be_bytes([header[1], header[2]]));
            let width = u32::from(u16::from_be_bytes([header[3], header[4]]));
            let components = header[5];
            let component_bytes = usize::from(components)
                .checked_mul(3)
                .and_then(|value| value.checked_add(6))
                .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
            if header.len() < component_bytes {
                return Err(BalancedCompressionSkipReason::CandidateDecode);
            }
            if precision != 8 || components != 3 {
                return Err(BalancedCompressionSkipReason::NonRgb8);
            }
            let pixels = u64::from(width)
                .checked_mul(u64::from(height))
                .filter(|pixels| *pixels <= MAX_IMAGE_PIXELS)
                .ok_or(BalancedCompressionSkipReason::CandidateDecode)?;
            if width == 0
                || height == 0
                || pixels == 0
                || width != expected_width
                || height != expected_height
            {
                return Err(BalancedCompressionSkipReason::CandidateDecode);
            }
            return Ok(());
        }
        cursor = segment_end;
    }
    Err(BalancedCompressionSkipReason::CandidateDecode)
}

fn copy_snapshot(
    source_path: &Path,
    destination_path: &Path,
    input: &JobInput,
    token: &CancellationToken,
    started: std::time::Instant,
) -> Result<(u64, String), OperationError> {
    let (canonical, identity) = canonical_regular_file(source_path).map_err(|_| input_error())?;
    let mut source = File::open(&canonical).map_err(|_| input_error())?;
    let metadata = source.metadata().map_err(|_| input_error())?;
    if identity.to_string() != input.file_identity
        || metadata.len() != input.size_bytes
        || modified_timestamp(&metadata)? != input.modified_at
    {
        return Err(source_changed_error());
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination_path)
        .map_err(|_| workspace_error())?;
    let mut hash = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_budget(token, started, OperationStage::Execute)?;
        let read = source.read(&mut buffer).map_err(|_| input_error())?;
        if read == 0 {
            break;
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|_| workspace_error())?;
        hash.update(&buffer[..read]);
        copied = copied.saturating_add(read as u64);
    }
    destination.sync_all().map_err(|_| workspace_error())?;
    if copied != input.size_bytes {
        return Err(source_changed_error());
    }
    Ok((copied, digest_hex(&hash.finalize())))
}

fn verify_source_metadata(input: &JobInput) -> Result<(), OperationError> {
    let (canonical, identity) =
        canonical_regular_file(Path::new(&input.canonical_path)).map_err(|_| input_error())?;
    let metadata = fs::metadata(canonical).map_err(|_| input_error())?;
    if identity.to_string() != input.file_identity
        || metadata.len() != input.size_bytes
        || modified_timestamp(&metadata)? != input.modified_at
    {
        return Err(source_changed_error());
    }
    Ok(())
}

fn verify_active_source(active: &ActiveBalancedJob) -> Result<(), OperationError> {
    let (canonical, identity) =
        canonical_regular_file(&active.source_path).map_err(|_| source_changed_error())?;
    let metadata = fs::metadata(&canonical).map_err(|_| source_changed_error())?;
    if identity.to_string() != active.source_identity
        || metadata.len() != active.source_size
        || modified_timestamp(&metadata)? != active.source_modified_at
        || hash_file(&canonical).map_err(|_| source_changed_error())?.1 != active.source_sha256
    {
        return Err(source_changed_error());
    }
    Ok(())
}

fn update_visual_audit(
    active: &mut ActiveBalancedJob,
    metrics: PageQualityMetrics,
) -> Result<(), OperationError> {
    active.comparison_pixels = active
        .comparison_pixels
        .checked_add(metrics.total_pixels)
        .filter(|pixels| *pixels <= BALANCED_COMPRESSION_MAX_TOTAL_PIXELS)
        .ok_or_else(resource_error)?;
    active.minimum_ssim = Some(
        active
            .minimum_ssim
            .map_or(metrics.ssim, |value| value.min(metrics.ssim)),
    );
    match metrics.psnr_db {
        None => {}
        Some(value) => {
            active.psnr_is_infinite = false;
            active.minimum_psnr_db = Some(
                active
                    .minimum_psnr_db
                    .map_or(value, |current| current.min(value)),
            );
        }
    }
    active.maximum_changed_pixels = active.maximum_changed_pixels.max(metrics.changed_pixels);
    active.maximum_total_pixels = active.maximum_total_pixels.max(metrics.total_pixels);
    Ok(())
}

fn audit_without_visual(prepared: &PreparedCandidate) -> BalancedCompressionAudit {
    BalancedCompressionAudit {
        profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
        source_bytes: prepared.source_size,
        candidate_bytes: prepared.candidate_size,
        saved_bytes: 0,
        saved_percent: 0.0,
        selected_images: 0,
        skipped_images: prepared.skipped.values().copied().sum(),
        affected_pages: 0,
        compared_pages: 0,
        minimum_ssim: None,
        minimum_psnr_db: None,
        psnr_is_infinite: false,
        maximum_changed_pixels: 0,
        maximum_total_pixels: 0,
        quality_passed: true,
        size_gate_passed: false,
        structural_proof_sha256: prepared.structural_proof_sha256.clone(),
        skipped_reasons: skip_counts(&prepared.skipped),
        created_at: timestamp(),
    }
}

fn skip_counts(skipped: &BTreeMap<String, u32>) -> Vec<BalancedCompressionSkipCount> {
    skipped
        .iter()
        .filter_map(|(reason, count)| {
            BalancedCompressionSkipReason::from_contract(reason).map(|reason| {
                BalancedCompressionSkipCount {
                    reason,
                    count: *count,
                }
            })
        })
        .collect()
}

fn add_skip(
    skipped: &mut BTreeMap<String, u32>,
    reason: BalancedCompressionSkipReason,
    count: u32,
) {
    if count > 0 {
        *skipped.entry(reason.as_str().to_owned()).or_default() += count;
    }
}

fn strip_length(value: &mut JsonValue) {
    if let Some(object) = value.as_object_mut() {
        object.remove("/Length");
    }
}

fn write_patch_entry(
    file: &mut File,
    reference: &str,
    candidate: &[u8],
    dictionary: &JsonMap<String, JsonValue>,
    first: bool,
) -> Result<(), OperationError> {
    if !first {
        file.write_all(b",").map_err(|_| workspace_error())?;
    }
    serde_json::to_writer(&mut *file, &format!("obj:{reference}"))
        .map_err(|_| workspace_error())?;
    file.write_all(b":{\"stream\":{\"data\":\"")
        .map_err(|_| workspace_error())?;
    {
        let mut encoder = EncoderWriter::new(&mut *file, &BASE64);
        encoder
            .write_all(candidate)
            .map_err(|_| workspace_error())?;
        encoder.finish().map_err(|_| workspace_error())?;
    }
    file.write_all(b"\",\"dict\":")
        .map_err(|_| workspace_error())?;
    serde_json::to_writer(&mut *file, dictionary).map_err(|_| workspace_error())?;
    file.write_all(b"}}").map_err(|_| workspace_error())
}

fn close_session(state: &AppState, session: &ViewerDocumentMetadata) -> Result<(), OperationError> {
    state.viewer_sessions.close(&ViewerSessionRequest {
        session_id: session.session_id.clone(),
        generation: session.generation,
    })
}

fn validate_request(request: &BalancedCompressionJobCreateRequest) -> Result<(), OperationError> {
    if request.operation_id != BALANCED_COMPRESSION_OPERATION_ID
        || request.settings.profile != BALANCED_COMPRESSION_PROFILE
        || request.input_paths.len() != 1
        || !request.input_paths[0]
            .to_ascii_lowercase()
            .ends_with(".pdf")
        || !request
            .requested_output_name
            .to_ascii_lowercase()
            .ends_with(".pdf")
    {
        return Err(request_error());
    }
    validate_output_name(&request.requested_output_name).map_err(|_| request_error())
}

fn document_savings_gate_passes(source_bytes: u64, candidate_bytes: u64) -> bool {
    source_bytes
        .checked_sub(candidate_bytes)
        .is_some_and(|saved| {
            saved >= MIN_DOCUMENT_SAVINGS && u128::from(saved) * 100 >= u128::from(source_bytes) * 5
        })
}

fn balanced_settings() -> JsonValue {
    json!({
        "profile": BALANCED_COMPRESSION_PROFILE,
        "jpegQuality": BALANCED_COMPRESSION_JPEG_QUALITY,
        "resampling": false,
        "renderer": { "id": "pdfjs-dist", "version": PDFJS_VERSION },
        "renderScale": 2,
        "background": "opaque-white",
        "publicationMinimumBytes": MIN_DOCUMENT_SAVINGS,
        "publicationMinimumPercent": 5,
    })
}

fn pdf_magic(file: &mut File) -> Result<bool, OperationError> {
    let mut bytes = [0_u8; 1024];
    let read = file.read(&mut bytes).map_err(|_| input_error())?;
    Ok(bytes[..read].windows(5).any(|window| window == b"%PDF-"))
}

fn modified_timestamp(metadata: &fs::Metadata) -> Result<String, OperationError> {
    let modified: chrono::DateTime<Utc> = metadata.modified().map_err(|_| input_error())?.into();
    Ok(modified.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn check_budget(
    token: &CancellationToken,
    started: std::time::Instant,
    stage: OperationStage,
) -> Result<(), OperationError> {
    check_cancelled(token, stage)?;
    if started.elapsed() > PREPARE_TIMEOUT {
        return Err(resource_error());
    }
    Ok(())
}

fn check_cancelled(token: &CancellationToken, stage: OperationStage) -> Result<(), OperationError> {
    if token.is_cancelled() {
        Err(OperationError::safe(
            "CANCELLED",
            "Balanced PDF Compression was cancelled",
            "Owned temporary data is being reconciled. No unverified output was published.",
            stage,
            false,
        ))
    } else {
        Ok(())
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
        job_id: job.id.clone(),
        operation_id: job.operation_id.clone(),
        sequence: job.sequence,
        state: job.state,
        stage,
        completed_units: job.progress.completed_units,
        total_units: job.progress.total_units,
        unit: job.progress.unit,
        message_code: message_code.to_owned(),
        message: message.to_owned(),
        cancellable,
        emitted_at: timestamp(),
    });
}

fn sha256_bytes(bytes: &[u8]) -> String {
    digest_hex(&Sha256::digest(bytes))
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn database_publication_error(_: crate::database::DatabaseError) -> PublicationError {
    publication_io_error()
}

fn publication_io_error() -> PublicationError {
    PublicationError::Io(std::io::Error::other(
        "balanced publication metadata failed",
    ))
}

fn publication_error(error: PublicationError) -> OperationError {
    match error {
        PublicationError::Cancelled => cancelled(),
        _ => OperationError::safe(
            "BALANCED_PUBLICATION_FAILED",
            "The balanced PDF could not be published safely",
            "No existing file was replaced. Owned temporary data will be reconciled.",
            OperationStage::Publish,
            true,
        ),
    }
}

fn request_error() -> OperationError {
    OperationError::safe(
        "BALANCED_REQUEST_INVALID",
        "The balanced compression request is invalid",
        "Use exactly one PDF and the fixed balanced-v1 profile.",
        OperationStage::Plan,
        false,
    )
}

fn path_error() -> OperationError {
    OperationError::safe(
        "BALANCED_PATH_INVALID",
        "A selected path is not safe",
        "Choose an existing local PDF and destination folder with a Windows-safe output name.",
        OperationStage::Inspect,
        false,
    )
}

fn input_error() -> OperationError {
    OperationError::safe(
        "BALANCED_INPUT_INVALID",
        "The selected file is not a valid local PDF",
        "Choose one regular, unencrypted PDF file.",
        OperationStage::Inspect,
        false,
    )
}

fn source_changed_error() -> OperationError {
    OperationError::safe(
        "BALANCED_SOURCE_CHANGED",
        "The source PDF changed during processing",
        "No output was published because the immutable source evidence no longer matched.",
        OperationStage::Verify,
        false,
    )
}

fn encryption_error() -> OperationError {
    OperationError::safe(
        "BALANCED_ENCRYPTED_PDF_REFUSED",
        "Encrypted PDFs are not supported",
        "Balanced compression does not decrypt documents or accept passwords.",
        OperationStage::Preflight,
        false,
    )
}

fn signature_error() -> OperationError {
    OperationError::safe(
        "BALANCED_SIGNED_PDF_REFUSED",
        "Signed PDFs are not supported",
        "Balanced compression refuses signature and ByteRange structures because rewriting would invalidate them.",
        OperationStage::Preflight,
        false,
    )
}

fn page_count_error() -> OperationError {
    OperationError::safe(
        "BALANCED_PAGE_COUNT_UNSUPPORTED",
        "The PDF has an unsupported page count",
        "Balanced compression supports PDFs with 1 through 4,096 pages.",
        OperationStage::Preflight,
        false,
    )
}

fn resource_error() -> OperationError {
    OperationError::safe(
        "BALANCED_RESOURCE_LIMIT",
        "The PDF exceeds a balanced-compression safety budget",
        "No output was published because the bounded graph, candidate, page, pixel, or time budget was exceeded.",
        OperationStage::Verify,
        false,
    )
}

fn inventory_error() -> OperationError {
    OperationError::safe(
        "BALANCED_INVENTORY_INVALID",
        "The PDF resource inventory could not be proven",
        "No output was published because the qpdf JSON structure was incomplete or ambiguous.",
        OperationStage::Verify,
        false,
    )
}

fn structural_error() -> OperationError {
    OperationError::safe(
        "BALANCED_STRUCTURE_CHANGED",
        "The candidate did not preserve PDF structure",
        "No output was published because protected objects or unselected streams could not be proven unchanged.",
        OperationStage::Verify,
        false,
    )
}

fn quality_error() -> OperationError {
    OperationError::safe(
        "BALANCED_VISUAL_QUALITY_FAILED",
        "An affected page did not meet the fixed quality thresholds",
        "No output was published because SSIM, PSNR, changed-pixel, or alpha verification failed.",
        OperationStage::Verify,
        false,
    )
}

fn stale_pixels() -> OperationError {
    OperationError::safe(
        "BALANCED_RENDER_STALE",
        "The page comparison data is stale or out of order",
        "No output was published because the authenticated source/candidate sequence did not match.",
        OperationStage::Verify,
        false,
    )
}

fn pixel_error() -> OperationError {
    OperationError::safe(
        "BALANCED_PIXEL_PAYLOAD_INVALID",
        "The rendered page pixels are invalid",
        "No output was published because the bounded RGBA8 opaque-white contract was not met.",
        OperationStage::Verify,
        false,
    )
}

fn dependency_error() -> OperationError {
    OperationError::safe(
        "BALANCED_DEPENDENCY_UNAVAILABLE",
        "A verified local PDF dependency is unavailable",
        "The bundled qpdf 12.3.2 and PDF.js 6.2.108 boundaries must be available.",
        OperationStage::Preflight,
        true,
    )
}

fn process_error(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "BALANCED_QPDF_FAILED",
        "The local PDF engine could not complete safely",
        "No output was published. The qpdf process was sandboxed and bounded.",
        stage,
        true,
    )
}

fn verify_error() -> OperationError {
    OperationError::safe(
        "BALANCED_VERIFICATION_FAILED",
        "The balanced candidate could not be verified",
        "No output was published because size, hash, PDF magic, page count, or source evidence did not match.",
        OperationStage::Verify,
        false,
    )
}

fn space_error() -> OperationError {
    OperationError::safe(
        "BALANCED_SPACE_INSUFFICIENT",
        "There is not enough local space",
        "Choose a destination with enough free space for verified temporary and publication copies.",
        OperationStage::Preflight,
        true,
    )
}

fn workspace_error() -> OperationError {
    OperationError::safe(
        "BALANCED_WORKSPACE_FAILED",
        "The private balanced-compression workspace is unavailable",
        "No user file was modified.",
        OperationStage::Execute,
        true,
    )
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "BALANCED_CLEANUP_UNPROVEN",
        "Temporary cleanup could not be proven",
        "The job was interrupted so recovery can reconcile only its marker-owned workspace.",
        OperationStage::Cleanup,
        true,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "BALANCED_METADATA_FAILED",
        "Balanced compression metadata could not be stored",
        "No unverified output was published.",
        OperationStage::Audit,
        true,
    )
}

fn cancelled() -> OperationError {
    OperationError::safe(
        "CANCELLED",
        "Balanced PDF Compression was cancelled",
        "Owned temporary data is being reconciled. No unverified output was published.",
        OperationStage::Cleanup,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replacement_without_cancellation(
        stream: &JsonMap<String, JsonValue>,
    ) -> Result<Option<ImageReplacement>, BalancedCompressionSkipReason> {
        let mut checkpoint = || Ok(());
        match replacement_for_stream(stream, &mut checkpoint) {
            Ok(replacement) => Ok(replacement),
            Err(ImageReplacementFailure::Skip(reason)) => Err(reason),
            Err(ImageReplacementFailure::Abort(error)) => {
                panic!("unexpected replacement abort: {}", error.code)
            }
        }
    }

    #[test]
    fn reference_parser_is_strict() {
        assert!(is_reference("12 0 R"));
        assert!(!is_reference("12 R"));
        assert!(!is_reference("12 0 R trailing"));
        assert!(!is_reference("-1 0 R"));
    }

    #[test]
    fn document_savings_gate_requires_both_exact_thresholds() {
        assert!(!document_savings_gate_passes(2_000_000, 1_900_001));
        assert!(!document_savings_gate_passes(1_000_000, 950_000));
        assert!(!document_savings_gate_passes(1_310_720, 1_245_185));
        assert!(document_savings_gate_passes(1_000_000, 934_464));
        assert!(document_savings_gate_passes(1_310_720, 1_245_184));
        assert!(!document_savings_gate_passes(1_000_000, 1_000_000));
        assert!(!document_savings_gate_passes(1_000_000, 1_000_001));
    }

    #[test]
    fn request_requires_one_pdf_and_the_fixed_profile() {
        let valid = BalancedCompressionJobCreateRequest {
            operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            input_paths: vec!["source.pdf".to_owned()],
            destination_directory: "destination".to_owned(),
            requested_output_name: "balanced.pdf".to_owned(),
            settings: crate::contracts::BalancedCompressionSettings {
                profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
            },
        };
        assert!(validate_request(&valid).is_ok());

        let mut wrong_profile = valid.clone();
        wrong_profile.settings.profile = "custom".to_owned();
        assert!(validate_request(&wrong_profile).is_err());

        let mut extra_input = valid.clone();
        extra_input.input_paths.push("second.pdf".to_owned());
        assert!(validate_request(&extra_input).is_err());

        let mut wrong_extension = valid;
        wrong_extension.input_paths[0] = "source.png".to_owned();
        assert!(validate_request(&wrong_extension).is_err());
    }

    #[test]
    fn selected_stream_inventory_ignores_only_allowed_fields() {
        let value = json!({"/Length": 9, "/Filter": "/DCTDecode", "/Width": 100});
        let mut stripped = value.clone();
        strip_length(&mut stripped);
        assert_eq!(stripped, json!({"/Filter": "/DCTDecode", "/Width": 100}));
    }

    #[test]
    fn image_allow_list_skips_protected_and_unsupported_classes() {
        for (label, amendment, expected) in [
            (
                "small",
                json!({"/Width": 255}),
                BalancedCompressionSkipReason::BelowMinimum,
            ),
            (
                "decode-array",
                json!({"/Decode": [0, 1, 0, 1, 0, 1]}),
                BalancedCompressionSkipReason::DecodeParameters,
            ),
            (
                "decode-parameters",
                json!({"/DecodeParms": {"/Predictor": 1}}),
                BalancedCompressionSkipReason::DecodeParameters,
            ),
            (
                "image-mask",
                json!({"/ImageMask": true}),
                BalancedCompressionSkipReason::MaskOrTransparency,
            ),
            (
                "soft-mask",
                json!({"/SMask": "9 0 R"}),
                BalancedCompressionSkipReason::MaskOrTransparency,
            ),
            (
                "explicit-mask",
                json!({"/Mask": [0, 1]}),
                BalancedCompressionSkipReason::MaskOrTransparency,
            ),
            (
                "icc",
                json!({"/ColorSpace": ["/ICCBased", "9 0 R"]}),
                BalancedCompressionSkipReason::UnsupportedColorspace,
            ),
            (
                "grayscale",
                json!({"/ColorSpace": "/DeviceGray"}),
                BalancedCompressionSkipReason::UnsupportedColorspace,
            ),
            (
                "cmyk",
                json!({"/ColorSpace": "/DeviceCMYK"}),
                BalancedCompressionSkipReason::UnsupportedColorspace,
            ),
            (
                "indexed",
                json!({"/ColorSpace": ["/Indexed", "/DeviceRGB", 255, "9 0 R"]}),
                BalancedCompressionSkipReason::UnsupportedColorspace,
            ),
            (
                "device-n",
                json!({"/ColorSpace": ["/DeviceN", ["/Spot"], "/DeviceCMYK", "9 0 R"]}),
                BalancedCompressionSkipReason::UnsupportedColorspace,
            ),
            (
                "non-eight-bit",
                json!({"/BitsPerComponent": 16}),
                BalancedCompressionSkipReason::NonRgb8,
            ),
            (
                "multi-filter",
                json!({"/Filter": ["/ASCII85Decode", "/DCTDecode"]}),
                BalancedCompressionSkipReason::UnsupportedFilter,
            ),
            (
                "jpx",
                json!({"/Filter": "/JPXDecode"}),
                BalancedCompressionSkipReason::UnsupportedFilter,
            ),
            (
                "jbig2",
                json!({"/Filter": "/JBIG2Decode"}),
                BalancedCompressionSkipReason::UnsupportedFilter,
            ),
            (
                "ccitt",
                json!({"/Filter": "/CCITTFaxDecode"}),
                BalancedCompressionSkipReason::UnsupportedFilter,
            ),
            (
                "external-stream",
                json!({"/F": "external.jpg"}),
                BalancedCompressionSkipReason::ExternalOrAlternate,
            ),
            (
                "alternate",
                json!({"/Alternates": []}),
                BalancedCompressionSkipReason::ExternalOrAlternate,
            ),
            (
                "opi",
                json!({"/OPI": {}}),
                BalancedCompressionSkipReason::ExternalOrAlternate,
            ),
        ] {
            let mut stream = json!({
                "dict": {
                    "/Type": "/XObject",
                    "/Subtype": "/Image",
                    "/Width": 512,
                    "/Height": 512,
                    "/ColorSpace": "/DeviceRGB",
                    "/BitsPerComponent": 8,
                    "/Filter": "/DCTDecode"
                },
                "data": ""
            });
            for (key, value) in amendment.as_object().unwrap() {
                stream["dict"][key] = value.clone();
            }
            assert_eq!(
                replacement_without_cancellation(stream.as_object().unwrap()).unwrap_err(),
                expected,
                "{label}"
            );
        }
    }

    #[test]
    fn safe_simple_flate_decodes_but_a_growing_candidate_is_not_selected() {
        let pixels = vec![0_u8; 512 * 512 * 3];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&pixels).unwrap();
        let encoded = encoder.finish().unwrap();
        let stream = json!({
            "dict": {
                "/Type": "/XObject",
                "/Subtype": "/Image",
                "/Width": 512,
                "/Height": 512,
                "/ColorSpace": "/DeviceRGB",
                "/BitsPerComponent": 8,
                "/Filter": "/FlateDecode"
            },
            "data": BASE64.encode(encoded)
        });
        assert_eq!(
            replacement_without_cancellation(stream.as_object().unwrap()).unwrap(),
            None
        );

        let oversized = vec![0_u8; 512 * 512 * 3 + 1];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&oversized).unwrap();
        let encoded = encoder.finish().unwrap();
        let mut oversized_stream = stream;
        oversized_stream["data"] = json!(BASE64.encode(encoded));
        assert_eq!(
            replacement_without_cancellation(oversized_stream.as_object().unwrap()).unwrap_err(),
            BalancedCompressionSkipReason::CandidateDecode
        );
    }

    #[test]
    fn dct_header_limits_and_rgb8_identity_are_proven_before_full_decode() {
        let oversized_header = vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x20, 0x00, 0x20, 0x00, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ];
        assert_eq!(
            verify_rgb8_jpeg_header(&oversized_header, 512, 512).unwrap_err(),
            BalancedCompressionSkipReason::CandidateDecode
        );

        let grayscale = vec![127_u8; 512 * 512];
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode(&grayscale, 512, 512, ExtendedColorType::L8)
            .unwrap();
        let stream = json!({
            "dict": {
                "/Type": "/XObject",
                "/Subtype": "/Image",
                "/Width": 512,
                "/Height": 512,
                "/ColorSpace": "/DeviceRGB",
                "/BitsPerComponent": 8,
                "/Filter": "/DCTDecode"
            },
            "data": BASE64.encode(jpeg)
        });
        assert_eq!(
            replacement_without_cancellation(stream.as_object().unwrap()).unwrap_err(),
            BalancedCompressionSkipReason::NonRgb8
        );
    }

    #[test]
    fn replacement_observes_cancellation_between_expensive_image_stages() {
        let pixels = vec![0_u8; 512 * 512 * 3];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&pixels).unwrap();
        let encoded = encoder.finish().unwrap();
        let stream = json!({
            "dict": {
                "/Type": "/XObject",
                "/Subtype": "/Image",
                "/Width": 512,
                "/Height": 512,
                "/ColorSpace": "/DeviceRGB",
                "/BitsPerComponent": 8,
                "/Filter": "/FlateDecode"
            },
            "data": BASE64.encode(encoded)
        });
        let mut checkpoints = 0_u32;
        let mut checkpoint = || {
            checkpoints += 1;
            if checkpoints == 2 {
                Err(cancelled())
            } else {
                Ok(())
            }
        };
        let result = replacement_for_stream(stream.as_object().unwrap(), &mut checkpoint);
        match result {
            Err(ImageReplacementFailure::Abort(error)) => assert_eq!(error.code, "CANCELLED"),
            other => panic!("expected cancellation after source decode, got {other:?}"),
        }
        assert_eq!(checkpoints, 2);
    }

    #[test]
    fn structural_inventory_binds_reference_topology_and_object_ownership() {
        let source = json!({
            "pages": [{"object": "1 0 R", "images": [], "outlines": null}],
            "qpdf": [{"jsonversion": 2}, {
                "obj:1 0 R": {"value": {"/Contents": "2 0 R", "/Thumb": "3 0 R"}},
                "obj:2 0 R": {"stream": {"dict": {"/Type": "/Metadata"}, "data": BASE64.encode(b"first")}},
                "obj:3 0 R": {"stream": {"dict": {"/Type": "/Metadata"}, "data": BASE64.encode(b"second")}},
                "trailer": {"value": {"/Root": "1 0 R"}}
            }]
        });
        let selected = BTreeSet::new();
        let source_inventory = structural_inventory(&source, &selected).unwrap();

        let mut changed_topology = source.clone();
        changed_topology["qpdf"][1]["obj:1 0 R"]["value"]["/Contents"] = json!("3 0 R");
        changed_topology["qpdf"][1]["obj:1 0 R"]["value"]["/Thumb"] = json!("2 0 R");
        assert_ne!(
            source_inventory,
            structural_inventory(&changed_topology, &selected).unwrap()
        );

        let mut changed_ownership = source.clone();
        changed_ownership["qpdf"][1]["obj:2 0 R"]["stream"]["data"] =
            source["qpdf"][1]["obj:3 0 R"]["stream"]["data"].clone();
        changed_ownership["qpdf"][1]["obj:3 0 R"]["stream"]["data"] =
            source["qpdf"][1]["obj:2 0 R"]["stream"]["data"].clone();
        assert_ne!(
            source_inventory,
            structural_inventory(&changed_ownership, &selected).unwrap()
        );

        let scoped_names = json!({
            "pages": [{
                "object": "1 0 R",
                "images": [
                    {"object": "5 0 R", "name": "/Im0", "filter": "/DCTDecode"},
                    {"object": "7 0 R", "name": "/Im0", "filter": "/DCTDecode"}
                ],
                "outlines": null
            }],
            "qpdf": [{"jsonversion": 2}, {
                "obj:1 0 R": {"value": {"/Resources": {"/XObject": {"/Fm0": "4 0 R", "/Fm1": "6 0 R"}}}},
                "obj:4 0 R": {"stream": {"dict": {"/Subtype": "/Form", "/Resources": {"/XObject": {"/Im0": "5 0 R"}}}, "data": BASE64.encode(b"form-zero")}},
                "obj:5 0 R": {"stream": {"dict": {"/Subtype": "/Image", "/Filter": "/DCTDecode"}, "data": BASE64.encode(b"selected-image")}},
                "obj:6 0 R": {"stream": {"dict": {"/Subtype": "/Form", "/Resources": {"/XObject": {"/Im0": "7 0 R"}}}, "data": BASE64.encode(b"form-one")}},
                "obj:7 0 R": {"stream": {"dict": {"/Subtype": "/Image", "/Filter": "/DCTDecode"}, "data": BASE64.encode(b"unselected-image")}},
                "trailer": {"value": {"/Root": "1 0 R"}}
            }]
        });
        let selected_reference = BTreeSet::from(["5 0 R".to_owned()]);
        let scoped_inventory = structural_inventory(&scoped_names, &selected_reference).unwrap();
        let mut allowed_selected_change = scoped_names.clone();
        allowed_selected_change["qpdf"][1]["obj:5 0 R"]["stream"]["data"] =
            json!(BASE64.encode(b"replacement-image"));
        assert_eq!(
            scoped_inventory,
            structural_inventory(&allowed_selected_change, &selected_reference).unwrap()
        );
        let mut forbidden_same_name_change = scoped_names;
        forbidden_same_name_change["qpdf"][1]["obj:7 0 R"]["stream"]["data"] =
            json!(BASE64.encode(b"mutated-unselected-image"));
        assert_ne!(
            scoped_inventory,
            structural_inventory(&forbidden_same_name_change, &selected_reference).unwrap()
        );
    }

    #[test]
    fn resource_graph_accepts_shared_and_nested_safe_uses_but_refuses_cycles() {
        let shared = graph_fixture(false, false);
        let objects = qpdf_objects(&shared).unwrap();
        let (uses, unsafe_images, _) = collect_safe_image_uses(&shared, objects).unwrap();
        let image_use = uses.get("5 0 R").unwrap();
        assert_eq!(image_use.page_indexes, BTreeSet::from([0, 1]));
        assert_eq!(image_use.safe_edges.len(), 2);
        assert!(unsafe_images.is_empty());

        let nested = graph_fixture(true, false);
        let objects = qpdf_objects(&nested).unwrap();
        let (uses, unsafe_images, _) = collect_safe_image_uses(&nested, objects).unwrap();
        assert_eq!(uses.get("5 0 R").unwrap().page_indexes, BTreeSet::from([0]));
        assert!(unsafe_images.is_empty());

        let transparent = graph_fixture(true, true);
        let objects = qpdf_objects(&transparent).unwrap();
        let (_, unsafe_images, _) = collect_safe_image_uses(&transparent, objects).unwrap();
        assert!(unsafe_images.contains("5 0 R"));

        let mut cyclic = graph_fixture(true, false);
        cyclic["qpdf"][1]["obj:6 0 R"]["stream"]["dict"]["/Resources"]["/XObject"]["/Self"] =
            json!("6 0 R");
        let objects = qpdf_objects(&cyclic).unwrap();
        assert!(collect_safe_image_uses(&cyclic, objects).is_err());

        let deep = deeply_nested_graph(MAX_GRAPH_DEPTH + 1);
        let objects = qpdf_objects(&deep).unwrap();
        assert!(collect_safe_image_uses(&deep, objects).is_err());

        let broad = broad_form_graph(MAX_GRAPH_OBJECTS + 1);
        let objects = qpdf_objects(&broad).unwrap();
        assert!(collect_safe_image_uses(&broad, objects).is_err());
    }

    fn deeply_nested_graph(form_count: usize) -> JsonValue {
        let mut objects = JsonMap::new();
        objects.insert(
            "obj:1 0 R".to_owned(),
            json!({"value": {"/Resources": {"/XObject": {"/F0": "10 0 R"}}}}),
        );
        for index in 0..form_count {
            let reference = 10 + index;
            let mut next = JsonMap::new();
            if index + 1 == form_count {
                next.insert("/Im0".to_owned(), json!("50 0 R"));
            } else {
                next.insert(
                    format!("/F{}", index + 1),
                    json!(format!("{} 0 R", reference + 1)),
                );
            }
            objects.insert(
                format!("obj:{reference} 0 R"),
                json!({"stream": {"dict": {
                    "/Subtype": "/Form",
                    "/Resources": {"/XObject": JsonValue::Object(next)}
                }, "data": ""}}),
            );
        }
        objects.insert(
            "obj:50 0 R".to_owned(),
            json!({"stream": {"dict": {"/Subtype": "/Image"}, "data": ""}}),
        );
        json!({
            "pages": [{"object": "1 0 R", "images": [{"object": "50 0 R", "name": "/Im0"}]}],
            "qpdf": [{"jsonversion": 2}, JsonValue::Object(objects)]
        })
    }

    fn broad_form_graph(form_count: usize) -> JsonValue {
        let mut objects = JsonMap::new();
        let mut xobjects = JsonMap::new();
        for index in 0..form_count {
            let reference = 10 + index;
            xobjects.insert(format!("/F{index}"), json!(format!("{reference} 0 R")));
            objects.insert(
                format!("obj:{reference} 0 R"),
                json!({"stream": {"dict": {"/Subtype": "/Form"}, "data": ""}}),
            );
        }
        objects.insert(
            "obj:1 0 R".to_owned(),
            json!({"value": {"/Resources": {"/XObject": JsonValue::Object(xobjects)}}}),
        );
        json!({
            "pages": [{"object": "1 0 R", "images": []}],
            "qpdf": [{"jsonversion": 2}, JsonValue::Object(objects)]
        })
    }

    fn graph_fixture(nested: bool, transparent: bool) -> JsonValue {
        let image = json!({
            "stream": {"dict": {"/Subtype": "/Image"}, "data": ""}
        });
        if nested {
            let mut fixture = json!({
                "pages": [{"object": "1 0 R", "images": [{"object": "5 0 R", "name": "/Im0"}]}],
                "qpdf": [{"jsonversion": 2}, {
                    "obj:1 0 R": {"value": {"/Resources": {"/XObject": {"/Fm0": "6 0 R"}}}},
                    "obj:5 0 R": image,
                    "obj:6 0 R": {"stream": {"dict": {
                        "/Subtype": "/Form",
                        "/Resources": {"/XObject": {"/Im0": "5 0 R"}}
                    }, "data": ""}}
                }]
            });
            if transparent {
                fixture["qpdf"][1]["obj:6 0 R"]["stream"]["dict"]["/Group"] =
                    json!({"/S": "/Transparency"});
            }
            return fixture;
        }
        json!({
            "pages": [
                {"object": "1 0 R", "images": [{"object": "5 0 R", "name": "/Im0"}]},
                {"object": "2 0 R", "images": [{"object": "5 0 R", "name": "/Im0"}]}
            ],
            "qpdf": [{"jsonversion": 2}, {
                "obj:1 0 R": {"value": {"/Resources": {"/XObject": {"/Im0": "5 0 R"}}}},
                "obj:2 0 R": {"value": {"/Resources": {"/XObject": {"/Im0": "5 0 R"}}}},
                "obj:5 0 R": image
            }]
        })
    }

    #[test]
    fn frozen_corpus_truthfully_reports_candidate_not_smaller() {
        let temporary = tempfile::tempdir().unwrap();
        let workspaces =
            crate::workspace::WorkspaceManager::initialize(&temporary.path().join("app-data"))
                .unwrap();
        let job_id = Uuid::new_v4().hyphenated().to_string();
        let workspace = workspaces.create_job(&job_id).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/g04c2-balanced-corpus/pdfs/g04c2-balanced-six-page.pdf");
        fs::copy(&fixture, workspace.root.join(SOURCE_RELATIVE)).unwrap();
        let runtime = crate::qpdf::QpdfRuntimeManager::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
            temporary.path().join("runtime-cache"),
        )
        .get_or_prepare()
        .unwrap();
        let profile = ensure_production_profile().unwrap();
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).unwrap();
        let registry = crate::app_state::CancellationRegistry::default();
        let token = registry.register(&job_id);
        let source_json =
            qpdf_json(&runtime, &workspace, Path::new(SOURCE_RELATIVE), &token).unwrap();
        let prepared = build_candidate(
            &runtime,
            &workspace,
            &source_json,
            6,
            &token,
            std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(prepared.selected_images, 0);
        assert!(prepared.affected_pages.is_empty());
        assert_eq!(prepared.skipped.get("candidate-not-smaller"), Some(&6));
        assert!(!workspace.root.join(CANDIDATE_RELATIVE).exists());
        assert_eq!(
            prepared.candidate_size,
            fs::metadata(fixture).unwrap().len()
        );
    }

    #[test]
    fn deterministic_rgb_fixture_uses_partial_json_and_preserves_structure() {
        let temporary = tempfile::tempdir().unwrap();
        let workspaces =
            crate::workspace::WorkspaceManager::initialize(&temporary.path().join("app-data"))
                .unwrap();
        let job_id = Uuid::new_v4().hyphenated().to_string();
        let workspace = workspaces.create_job(&job_id).unwrap();
        let source = synthetic_rgb_pdf(512, 512, 100);
        fs::write(workspace.root.join(SOURCE_RELATIVE), source).unwrap();

        let runtime = crate::qpdf::QpdfRuntimeManager::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
            temporary.path().join("runtime-cache"),
        )
        .get_or_prepare()
        .unwrap();
        let profile = ensure_production_profile().unwrap();
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).unwrap();
        let registry = crate::app_state::CancellationRegistry::default();
        let token = registry.register(&job_id);
        let source_json =
            qpdf_json(&runtime, &workspace, Path::new(SOURCE_RELATIVE), &token).unwrap();
        let prepared = build_candidate(
            &runtime,
            &workspace,
            &source_json,
            1,
            &token,
            std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(prepared.selected_images, 1, "{:?}", prepared.skipped);
        assert_eq!(prepared.affected_pages, vec![0]);
        assert!(prepared.candidate_size > 0);
        assert!(
            prepared.candidate_size
                < fs::metadata(workspace.root.join(SOURCE_RELATIVE))
                    .unwrap()
                    .len()
        );
        assert!(workspace.root.join(CANDIDATE_RELATIVE).is_file());

        let second_job_id = Uuid::new_v4().hyphenated().to_string();
        let second_workspace = workspaces.create_job(&second_job_id).unwrap();
        fs::copy(
            workspace.root.join(SOURCE_RELATIVE),
            second_workspace.root.join(SOURCE_RELATIVE),
        )
        .unwrap();
        authorize_qpdf_paths(&profile, &runtime.bin, &second_workspace).unwrap();
        let second_token = registry.register(&second_job_id);
        let second_json = qpdf_json(
            &runtime,
            &second_workspace,
            Path::new(SOURCE_RELATIVE),
            &second_token,
        )
        .unwrap();
        let second = build_candidate(
            &runtime,
            &second_workspace,
            &second_json,
            1,
            &second_token,
            std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(prepared.candidate_size, second.candidate_size);
        assert_eq!(prepared.candidate_sha256, second.candidate_sha256);
        assert_eq!(
            fs::read(workspace.root.join(CANDIDATE_RELATIVE)).unwrap(),
            fs::read(second_workspace.root.join(CANDIDATE_RELATIVE)).unwrap()
        );
    }

    #[test]
    fn service_publishes_only_after_visual_and_both_size_gates() {
        let app_data = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = source_root.path().join("beneficial.pdf");
        fs::write(&source_path, synthetic_rgb_pdf(2_048, 2_048, 100)).unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let request = BalancedCompressionJobCreateRequest {
            operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            input_paths: vec![source_path.to_string_lossy().into_owned()],
            destination_directory: destination.path().to_string_lossy().into_owned(),
            requested_output_name: "balanced.pdf".to_owned(),
            settings: crate::contracts::BalancedCompressionSettings {
                profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
            },
        };
        let job = service.create_job(request).unwrap();
        let token = state.cancellations.register(&job.id);
        let mut visual = None;
        let prepared = service
            .prepare_with_registered_token(&job.id, token, |_| {}, |session| visual = Some(session))
            .unwrap();
        assert_eq!(prepared.state, JobState::Verifying);
        assert!(prepared.outputs.is_empty());
        let visual = visual.expect("a safe replacement must require the page visual gate");
        assert_eq!(visual.pages.len(), 1);
        let ticket = visual.pages[0].clone();
        let rgba = vec![255_u8; 16 * 16 * 4];
        let source_result = service
            .submit_page(
                BalancedPixelUploadMetadata {
                    job_id: job.id.clone(),
                    render_session_id: visual.render_session_id.clone(),
                    page_ordinal: ticket.page_ordinal,
                    source_page_index: ticket.source_page_index,
                    nonce: ticket.nonce.clone(),
                    side: BalancedRenderSide::Source,
                    expected_width: 16,
                    expected_height: 16,
                },
                rgba.clone(),
                |_| {},
            )
            .unwrap();
        assert_eq!(source_result.state, JobState::Verifying);
        let completed = service
            .submit_page(
                BalancedPixelUploadMetadata {
                    job_id: job.id.clone(),
                    render_session_id: visual.render_session_id,
                    page_ordinal: ticket.page_ordinal,
                    source_page_index: ticket.source_page_index,
                    nonce: ticket.nonce,
                    side: BalancedRenderSide::Candidate,
                    expected_width: 16,
                    expected_height: 16,
                },
                rgba,
                |_| {},
            )
            .unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(
            completed.completion_kind,
            Some(crate::contracts::JobCompletionKind::Published)
        );
        assert_eq!(completed.outputs.len(), 1);
        let output = PathBuf::from(completed.outputs[0].final_path.as_ref().unwrap());
        assert!(output.is_file());
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert!(!state.workspaces.root().join(&job.id).exists());
        let audit = state
            .database()
            .get_balanced_compression_audit(&job.id)
            .unwrap()
            .unwrap();
        assert!(audit.quality_passed);
        assert!(audit.size_gate_passed);
        assert!(audit.saved_bytes >= MIN_DOCUMENT_SAVINGS);
        assert!(u128::from(audit.saved_bytes) * 100 >= u128::from(audit.source_bytes) * 5);
    }

    #[test]
    fn frozen_corpus_completes_no_benefit_without_visual_or_output() {
        let app_data = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/g04c2-balanced-corpus/pdfs/g04c2-balanced-six-page.pdf");
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![source_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        let mut visual_emitted = false;
        let completed = service
            .prepare_with_registered_token(&job.id, token, |_| {}, |_| visual_emitted = true)
            .unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(
            completed.completion_kind,
            Some(crate::contracts::JobCompletionKind::NoBenefit)
        );
        assert_eq!(
            completed.reason,
            Some(crate::contracts::JobCompletionReason::SavingsThresholdNotMet)
        );
        assert!(completed.outputs.is_empty());
        assert!(completed.errors.is_empty());
        assert!(!visual_emitted);
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
        let audit = state
            .database()
            .get_balanced_compression_audit(&job.id)
            .unwrap()
            .unwrap();
        assert_eq!(audit.selected_images, 0);
        assert_eq!(audit.skipped_images, 6);
        assert!(!audit.size_gate_passed);
    }

    #[test]
    fn signed_and_tampered_spec_jobs_fail_closed_without_output() {
        for (case, source_bytes, tamper_spec, expected_code) in [
            (
                "signed",
                synthetic_rgb_pdf_with_catalog(512, 512, 100, "/ByteRange [0 1 2 3]"),
                false,
                "BALANCED_SIGNED_PDF_REFUSED",
            ),
            (
                "spec-tamper",
                synthetic_rgb_pdf(512, 512, 100),
                true,
                "BALANCED_METADATA_FAILED",
            ),
        ] {
            let app_data = tempfile::tempdir().unwrap();
            let source_root = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            let source_path = source_root.path().join(format!("{case}.pdf"));
            fs::write(&source_path, source_bytes).unwrap();
            let database =
                crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
            let workspaces =
                crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
            let state = AppState::new(database, workspaces).with_qpdf(
                crate::qpdf::QpdfRuntimeManager::new(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                    app_data.path().join("runtime-cache"),
                ),
            );
            let service = BalancedCompressionService::new(state.clone());
            let job = service
                .create_job(BalancedCompressionJobCreateRequest {
                    operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                    input_paths: vec![source_path.to_string_lossy().into_owned()],
                    destination_directory: destination.path().to_string_lossy().into_owned(),
                    requested_output_name: "must-not-exist.pdf".to_owned(),
                    settings: crate::contracts::BalancedCompressionSettings {
                        profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                    },
                })
                .unwrap();
            if tamper_spec {
                state
                    .database()
                    .connection()
                    .execute(
                        "UPDATE job_operation_specs SET settings_sha256 = ?1 WHERE job_id = ?2",
                        ("0".repeat(64), &job.id),
                    )
                    .unwrap();
            }
            let token = state.cancellations.register(&job.id);
            let error = service
                .prepare_with_registered_token(&job.id, token, |_| {}, |_| {})
                .unwrap_err();
            let failed = state.database().get_job(&job.id).unwrap().unwrap();
            assert_eq!(
                error.code, expected_code,
                "{case}: state={:?}, stored_errors={:?}",
                failed.state, failed.errors
            );
            assert_eq!(failed.state, JobState::Failed, "{case}");
            assert!(failed.outputs.is_empty(), "{case}");
            assert!(
                !destination.path().join("must-not-exist.pdf").exists(),
                "{case}"
            );
            assert!(!state.workspaces.root().join(&job.id).exists(), "{case}");
        }
    }

    #[test]
    fn encrypted_pdf_is_refused_before_strict_content_inspection() {
        let app_data = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let plain_path = source_root.path().join("plain.pdf");
        let encrypted_path = source_root.path().join("encrypted.pdf");
        fs::write(&plain_path, synthetic_rgb_pdf(512, 512, 100)).unwrap();
        let qpdf = crate::qpdf::QpdfRuntimeManager::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
            app_data.path().join("runtime-cache"),
        );
        let runtime = qpdf.get_or_prepare().unwrap();
        let status = std::process::Command::new(&runtime.executable)
            .arg("--encrypt")
            .arg("")
            .arg("owner-password")
            .arg("256")
            .arg("--")
            .arg(&plain_path)
            .arg(&encrypted_path)
            .status()
            .unwrap();
        assert!(status.success());

        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state = AppState::new(database, workspaces).with_qpdf(qpdf);
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![encrypted_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        let error = service
            .prepare_with_registered_token(&job.id, token, |_| {}, |_| {})
            .unwrap_err();
        assert_eq!(error.code, "BALANCED_ENCRYPTED_PDF_REFUSED");
        let failed = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert!(failed.outputs.is_empty());
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }

    #[test]
    fn changed_source_is_refused_before_candidate_creation() {
        let app_data = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = source_root.path().join("changed.pdf");
        fs::write(&source_path, synthetic_rgb_pdf(512, 512, 100)).unwrap();
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![source_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        fs::write(&source_path, synthetic_rgb_pdf(513, 512, 100)).unwrap();
        let token = state.cancellations.register(&job.id);
        let error = service
            .prepare_with_registered_token(&job.id, token, |_| {}, |_| {})
            .unwrap_err();
        assert_eq!(error.code, "BALANCED_SOURCE_CHANGED");
        let failed = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert!(failed.outputs.is_empty());
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }

    #[test]
    fn stale_visual_upload_fails_closed_and_cleans_private_candidate() {
        let app_data = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = source_root.path().join("stale.pdf");
        fs::write(&source_path, synthetic_rgb_pdf(2_048, 2_048, 100)).unwrap();
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![source_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        let mut visual = None;
        service
            .prepare_with_registered_token(&job.id, token, |_| {}, |session| visual = Some(session))
            .unwrap();
        let visual = visual.unwrap();
        let ticket = visual.pages[0].clone();
        let error = service
            .submit_page(
                BalancedPixelUploadMetadata {
                    job_id: job.id.clone(),
                    render_session_id: Uuid::new_v4().hyphenated().to_string(),
                    page_ordinal: ticket.page_ordinal,
                    source_page_index: ticket.source_page_index,
                    nonce: ticket.nonce,
                    side: BalancedRenderSide::Source,
                    expected_width: 16,
                    expected_height: 16,
                },
                vec![255_u8; 16 * 16 * 4],
                |_| {},
            )
            .unwrap_err();
        assert_eq!(error.code, "BALANCED_RENDER_STALE");
        let failed = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert!(failed.outputs.is_empty());
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }

    #[test]
    fn candidate_visual_geometry_must_match_the_authenticated_source_geometry() {
        let app_data = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = source_root.path().join("geometry.pdf");
        fs::write(&source_path, synthetic_rgb_pdf(512, 512, 100)).unwrap();
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![source_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        let mut visual = None;
        service
            .prepare_with_registered_token(&job.id, token, |_| {}, |session| visual = Some(session))
            .unwrap();
        let visual = visual.unwrap();
        let ticket = visual.pages[0].clone();
        let source_pixels = vec![255_u8; 16 * 32 * 4];
        service
            .submit_page(
                BalancedPixelUploadMetadata {
                    job_id: job.id.clone(),
                    render_session_id: visual.render_session_id.clone(),
                    page_ordinal: ticket.page_ordinal,
                    source_page_index: ticket.source_page_index,
                    nonce: ticket.nonce.clone(),
                    side: BalancedRenderSide::Source,
                    expected_width: 16,
                    expected_height: 32,
                },
                source_pixels.clone(),
                |_| {},
            )
            .unwrap();
        let error = service
            .submit_page(
                BalancedPixelUploadMetadata {
                    job_id: job.id.clone(),
                    render_session_id: visual.render_session_id,
                    page_ordinal: ticket.page_ordinal,
                    source_page_index: ticket.source_page_index,
                    nonce: ticket.nonce,
                    side: BalancedRenderSide::Candidate,
                    expected_width: 32,
                    expected_height: 16,
                },
                source_pixels,
                |_| {},
            )
            .unwrap_err();
        assert_eq!(error.code, "BALANCED_RENDER_STALE");
        let failed = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert!(failed.outputs.is_empty());
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }

    #[test]
    fn preflight_cancellation_wins_over_no_benefit_completion() {
        let app_data = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/g04c2-balanced-corpus/pdfs/g04c2-balanced-six-page.pdf");
        let database =
            crate::database::Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
        let workspaces = crate::workspace::WorkspaceManager::initialize(app_data.path()).unwrap();
        let state =
            AppState::new(database, workspaces).with_qpdf(crate::qpdf::QpdfRuntimeManager::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
                app_data.path().join("runtime-cache"),
            ));
        let service = BalancedCompressionService::new(state.clone());
        let job = service
            .create_job(BalancedCompressionJobCreateRequest {
                operation_id: BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
                input_paths: vec![source_path.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "must-not-exist.pdf".to_owned(),
                settings: crate::contracts::BalancedCompressionSettings {
                    profile: BALANCED_COMPRESSION_PROFILE.to_owned(),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        assert_eq!(
            state.cancellations.request(&job.id),
            crate::app_state::CancelOutcome::Requested
        );
        state.database().request_cancellation(&job.id).unwrap();
        let error = service
            .prepare_with_registered_token(&job.id, token, |_| {}, |_| {})
            .unwrap_err();
        assert_eq!(error.code, "CANCELLED");
        let cancelled = state.database().get_job(&job.id).unwrap().unwrap();
        assert_eq!(cancelled.state, JobState::Cancelled);
        assert!(cancelled.outputs.is_empty());
        assert!(!destination.path().join("must-not-exist.pdf").exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }

    fn synthetic_rgb_pdf(width: u32, height: u32, quality: u8) -> Vec<u8> {
        synthetic_rgb_pdf_with_catalog(width, height, quality, "")
    }

    fn synthetic_rgb_pdf_with_catalog(
        width: u32,
        height: u32,
        quality: u8,
        catalog_extra: &str,
    ) -> Vec<u8> {
        let decoded = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                ((u64::from(x) * 255) / u64::from(width.max(1))) as u8,
                ((u64::from(y) * 255) / u64::from(height.max(1))) as u8,
                (((u64::from(x) + u64::from(y)) * 127) / u64::from(width.max(1) + height.max(1)))
                    as u8,
            ])
        });
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode(
                decoded.as_raw(),
                decoded.width(),
                decoded.height(),
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        single_image_pdf_with_catalog(&jpeg, width, height, catalog_extra)
    }

    fn single_image_pdf_with_catalog(
        jpeg: &[u8],
        width: u32,
        height: u32,
        catalog_extra: &str,
    ) -> Vec<u8> {
        let content = format!("q\n{width} 0 0 {height} 0 0 cm\n/Im0 Do\nQ\n");
        let objects = [
            format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
            )
            .into_bytes(),
            [
                format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(),
                content.into_bytes(),
                b"endstream".to_vec(),
            ]
            .concat(),
            [
                format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                    jpeg.len()
                )
                .into_bytes(),
                jpeg.to_vec(),
                b"\nendstream".to_vec(),
            ]
            .concat(),
        ];
        let mut pdf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }
}
