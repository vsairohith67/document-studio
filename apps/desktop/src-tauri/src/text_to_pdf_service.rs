use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
};

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    DependencyDiagnostic, DependencyKind, DependencyStatus, JobInput, JobOutput, JobProgress,
    JobRecord, JobState, OperationError, OperationSpecEnvelope, OperationStage, OutputStatus,
    ProgressEvent, ProgressUnit, StoredOperationSpec, TextToPdfJobCreateRequest,
    OPERATION_SPEC_SCHEMA_VERSION,
};
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, reject_reparse_components,
    validate_output_name,
};
use crate::pdf_merge::{run_qpdf, run_qpdf_with_capture_limit};
use crate::process_sandbox::{authorize_qpdf_paths, ensure_production_profile};
use crate::publication::{
    hash_file, is_exact_owned_partial_path, partial_ownership_result_code,
    publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::qpdf::{
    build_text_pdf_normalization_arguments, interpret_encryption_check_exit,
    interpret_structural_check_exit, EncryptionCheckOutcome, StructuralCheckOutcome,
    TEXT_NORMALIZED_STAGING_RELATIVE_PATH, TEXT_RAW_STAGING_RELATIVE_PATH,
};
use crate::text_to_pdf::{
    canonical_css, canonical_html, preflight_text, total_response_bytes, validate_approved_fonts,
    AdmittedScript, TextToPdfSettings, TEXT_TO_PDF_OPERATION_ID, TEXT_TO_PDF_VERSION,
    TXT_MAX_PAGES, TXT_MAX_PDF_BYTES, TXT_MAX_RAW_BYTES,
};
use crate::text_to_pdf_renderer::{render_text_pdf, TextRenderEvidence, TextRenderRequest};
use crate::viewer_sessions::ViewerJobSource;
use crate::windows_security::{delete_open_file, identity_from_file, open_for_identity_and_delete};
use crate::workspace::JobWorkspace;

const QPDF_NORMALIZE_TIMEOUT: Duration = Duration::from_secs(300);
const QPDF_VERIFY_TIMEOUT: Duration = Duration::from_secs(300);
const QPDF_JSON_CAPTURE_LIMIT: usize = 32 * 1024 * 1024;
const TOTAL_OPERATION_TIMEOUT: Duration = Duration::from_secs(600);
const UDF_MARKER: &str = ".document-studio-txt-renderer-v1";
const TOTAL_PROGRESS_STEPS: u64 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceCheckpoint {
    QpdfNormalization,
    OutputVerification,
    PrePublication,
    PostPublicationAudit,
    Cleanup,
}

#[cfg(test)]
static SERVICE_TEST_CANCELLATION: OnceLock<Mutex<Option<(String, ServiceCheckpoint)>>> =
    OnceLock::new();

#[cfg(test)]
static PUBLICATION_COMMIT_TEST_DELAY: OnceLock<Mutex<Option<(String, Duration)>>> = OnceLock::new();

#[derive(Clone)]
pub struct TextToPdfService {
    state: AppState,
}

#[derive(Debug)]
struct VerifiedPdf {
    size: u64,
    sha256: String,
    pages: u64,
}

#[derive(Debug)]
struct RendererWorkspace {
    path: PathBuf,
    marker_value: String,
    generation: String,
}

impl TextToPdfService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn create_job(
        &self,
        request: TextToPdfJobCreateRequest,
    ) -> Result<JobRecord, OperationError> {
        if request.operation_id != TEXT_TO_PDF_OPERATION_ID {
            return Err(invalid_request());
        }
        validate_output_name(&request.requested_output_name).map_err(|_| invalid_output_name())?;
        if !request
            .requested_output_name
            .to_ascii_lowercase()
            .ends_with(".pdf")
        {
            return Err(invalid_output_name());
        }
        let source = self
            .state
            .viewer_sessions
            .source_for_text_job(&request.input_session_id, request.input_generation)?;
        if source.size_bytes > TXT_MAX_RAW_BYTES as u64 {
            return Err(size_limit());
        }
        let destination = self
            .state
            .viewer_sessions
            .resolve_destination(&request.destination_grant_id)?;
        ensure_different_files(
            &source.path,
            &destination.join(&request.requested_output_name),
        )
        .map_err(|_| path_error())?;

        let id = Uuid::new_v4().hyphenated().to_string();
        let now = timestamp();
        let settings = serde_json::to_value(request.settings).map_err(|_| metadata_error())?;
        let job = JobRecord {
            id,
            operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
            operation_version: TEXT_TO_PDF_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: TOTAL_PROGRESS_STEPS,
                unit: ProgressUnit::Steps,
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
                display_name: "Selected TXT".to_owned(),
                source_path: String::new(),
                canonical_path: String::new(),
                file_identity: source.file_identity.clone(),
                size_bytes: source.size_bytes,
                modified_at: source.modified_at.clone(),
                mime_type: "text/plain".to_owned(),
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
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                status: OutputStatus::Planned,
                verified_at: None,
                published_at: None,
            }],
            errors: Vec::new(),
        };
        let envelope = OperationSpecEnvelope {
            schema_version: OPERATION_SPEC_SCHEMA_VERSION,
            operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
            settings,
        };
        let canonical_json = serde_json::to_string(&envelope).map_err(|_| metadata_error())?;
        let spec = StoredOperationSpec {
            sha256: sha256_hex(canonical_json.as_bytes()),
            canonical_json,
            envelope,
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
        source: ViewerJobSource,
        token: CancellationToken,
        on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.execute_registered(job_id, source, token, on_event)
    }

    fn execute_registered<F>(
        &self,
        job_id: &str,
        source: ViewerJobSource,
        token: CancellationToken,
        mut on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let operation_deadline = Instant::now()
            .checked_add(TOTAL_OPERATION_TIMEOUT)
            .ok_or_else(|| operation_timeout(OperationStage::Inspect))?;
        let mut workspace = None;
        let mut renderer_workspace = None;
        let result = self.execute_inner(
            job_id,
            &source,
            &token,
            &mut workspace,
            &mut renderer_workspace,
            operation_deadline,
            &mut on_event,
        );
        let result = match result {
            Ok(job) => Ok(job),
            Err(error) if error.code == "CANCELLED" => self.finish_cancelled(
                job_id,
                workspace.as_ref(),
                renderer_workspace.as_ref(),
                &mut on_event,
            ),
            Err(error) => {
                self.finish_failed(
                    job_id,
                    workspace.as_ref(),
                    renderer_workspace.as_ref(),
                    &error,
                    &mut on_event,
                )?;
                Err(error)
            }
        };
        self.state.cancellations.unregister(job_id);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_inner<F>(
        &self,
        job_id: &str,
        source: &ViewerJobSource,
        token: &CancellationToken,
        workspace_slot: &mut Option<JobWorkspace>,
        renderer_workspace_slot: &mut Option<RendererWorkspace>,
        operation_deadline: Instant,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_deadline(operation_deadline, OperationStage::Inspect)?;
        let inspecting = self.transition(
            job_id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )?;
        self.progress(
            job_id,
            JobState::Inspecting,
            OperationStage::Inspect,
            1,
            TOTAL_PROGRESS_STEPS,
            "TXT_INSPECTING",
            "Checking the selected local TXT document",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Inspect)?;
        check_deadline(operation_deadline, OperationStage::Inspect)?;
        let input = inspecting.inputs.first().ok_or_else(metadata_error)?;
        if input.file_identity != source.file_identity
            || input.size_bytes != source.size_bytes
            || input.modified_at != source.modified_at
            || input.mime_type != "text/plain"
        {
            return Err(source_changed(OperationStage::Inspect));
        }
        let (raw, source_hash) = source.read_all_bounded(TXT_MAX_RAW_BYTES, token)?;
        self.state
            .database()
            .update_input_hash(job_id, 0, &source_hash)
            .map_err(|_| metadata_error())?;

        self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Preflight,
            2,
            TOTAL_PROGRESS_STEPS,
            "TXT_PREFLIGHT",
            "Validating UTF-8, Unicode shaping bounds, fonts, and private response limits",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Preflight)?;
        check_deadline(operation_deadline, OperationStage::Preflight)?;
        let spec = self
            .state
            .database()
            .get_operation_spec(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if spec.envelope.schema_version != OPERATION_SPEC_SCHEMA_VERSION
            || spec.envelope.operation_id != TEXT_TO_PDF_OPERATION_ID
        {
            return Err(metadata_error());
        }
        let settings: TextToPdfSettings =
            serde_json::from_value(spec.envelope.settings).map_err(|_| metadata_error())?;
        let normalized = preflight_text(&raw)?;
        drop(raw);
        validate_approved_fonts()?;
        let html = canonical_html(&normalized.text)?;
        let css = canonical_css()?;
        total_response_bytes(html.len(), css.len())?;
        let html: std::sync::Arc<[u8]> = html.into();
        let css: std::sync::Arc<[u8]> = css.into();
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
        *workspace_slot = Some(workspace.clone());
        let profile = ensure_production_profile().map_err(|_| dependency_error())?;
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| dependency_error())?;
        verify_qpdf_version_bounded(&runtime, &workspace, token, operation_deadline)?;
        let renderer_workspace = create_renderer_workspace(&workspace, job_id)?;
        *renderer_workspace_slot = Some(RendererWorkspace {
            path: renderer_workspace.path.clone(),
            marker_value: renderer_workspace.marker_value.clone(),
            generation: renderer_workspace.generation.clone(),
        });
        self.transition(
            job_id,
            JobState::Preflight,
            JobState::Ready,
            OperationStage::Plan,
        )?;
        self.progress(
            job_id,
            JobState::Ready,
            OperationStage::Plan,
            3,
            TOTAL_PROGRESS_STEPS,
            "TXT_READY",
            "The bounded local TXT-to-PDF plan is ready",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Plan)?;
        check_deadline(operation_deadline, OperationStage::Plan)?;

        self.transition(
            job_id,
            JobState::Ready,
            JobState::Running,
            OperationStage::Execute,
        )?;
        self.progress(
            job_id,
            JobState::Running,
            OperationStage::Execute,
            4,
            TOTAL_PROGRESS_STEPS,
            "TXT_RENDERING",
            "Rendering the private document with the controlled static fonts",
            true,
            on_event,
        )?;
        let running = self.current_job(job_id)?;
        let raw_pdf_path = workspace.root.join(TEXT_RAW_STAGING_RELATIVE_PATH);
        let render_evidence = render_text_pdf(
            TextRenderRequest {
                job_id: job_id.to_owned(),
                renderer_generation: renderer_workspace.generation.clone(),
                lifecycle_version: running.version,
                user_data_directory: renderer_workspace.path.clone(),
                raw_pdf_path: raw_pdf_path.clone(),
                html,
                css,
                used_scripts: normalized.used_scripts.clone(),
                settings,
                operation_deadline,
            },
            token.clone(),
        )?;
        self.state
            .database()
            .upsert_dependency(&DependencyDiagnostic {
                id: "webview2".to_owned(),
                kind: DependencyKind::External,
                status: DependencyStatus::Available,
                version: Some(render_evidence.runtime_version.clone()),
                capabilities: vec![TEXT_TO_PDF_OPERATION_ID.to_owned()],
                checked_at: timestamp(),
                error_code: None,
            })
            .map_err(|_| metadata_error())?;
        drop(normalized.text);
        check_cancelled(token, OperationStage::Execute)?;
        check_deadline(operation_deadline, OperationStage::Execute)?;

        self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        self.progress(
            job_id,
            JobState::Verifying,
            OperationStage::Verify,
            12,
            TOTAL_PROGRESS_STEPS,
            "TXT_VERIFYING_RAW",
            "Verifying and normalizing the private WebView2 PDF",
            true,
            on_event,
        )?;
        let verifying = self.current_job(job_id)?;
        let raw_verified = verify_pdf_basics(
            &runtime,
            &workspace,
            Path::new(TEXT_RAW_STAGING_RELATIVE_PATH),
            token,
            operation_deadline,
        )?;
        let normalize_arguments = build_text_pdf_normalization_arguments(
            Path::new(TEXT_RAW_STAGING_RELATIVE_PATH),
            Path::new(TEXT_NORMALIZED_STAGING_RELATIVE_PATH),
        )
        .map_err(|_| verify_error("TXT_QPDF_ARGUMENTS_INVALID"))?;
        service_test_checkpoint(&self.state, job_id, ServiceCheckpoint::QpdfNormalization);
        let normalized_execution = run_qpdf(
            &runtime,
            &workspace,
            &normalize_arguments,
            token,
            bounded_timeout(
                operation_deadline,
                QPDF_NORMALIZE_TIMEOUT,
                OperationStage::Verify,
            )?,
            OperationStage::Verify,
        )?;
        if normalized_execution.exit_code != 0 {
            return Err(verify_error("TXT_QPDF_NORMALIZE_FAILED"));
        }
        let normalized_path = workspace.root.join(TEXT_NORMALIZED_STAGING_RELATIVE_PATH);
        let normalized_verified = verify_pdf_basics(
            &runtime,
            &workspace,
            Path::new(TEXT_NORMALIZED_STAGING_RELATIVE_PATH),
            token,
            operation_deadline,
        )?;
        if raw_verified.pages != normalized_verified.pages {
            return Err(verify_error("TXT_PAGE_COUNT_MISMATCH"));
        }
        service_test_checkpoint(&self.state, job_id, ServiceCheckpoint::OutputVerification);
        check_cancelled(token, OperationStage::Verify)?;
        inspect_pdf_security(
            &runtime,
            &workspace,
            Path::new(TEXT_NORMALIZED_STAGING_RELATIVE_PATH),
            settings,
            &normalized.used_scripts,
            source,
            &renderer_workspace,
            token,
            operation_deadline,
        )?;
        check_deadline(operation_deadline, OperationStage::Verify)?;
        source.verify_unchanged_hash(&source_hash, token)?;
        self.state
            .database()
            .set_output_staging(
                job_id,
                &normalized_path.to_string_lossy(),
                normalized_verified.size,
                &normalized_verified.sha256,
                &timestamp(),
            )
            .map_err(|_| metadata_error())?;
        fs::remove_file(&raw_pdf_path).map_err(|_| cleanup_error())?;
        write_private_audit(
            &workspace,
            settings,
            &render_evidence,
            &source_hash,
            &raw_verified,
            &normalized_verified,
        )?;

        let destination = canonical_directory(Path::new(&verifying.destination_directory))
            .map_err(|_| path_error())?;
        service_test_checkpoint(&self.state, job_id, ServiceCheckpoint::PrePublication);
        self.publish(
            job_id,
            &verifying,
            &normalized_path,
            &destination,
            source,
            normalized_verified.size,
            &normalized_verified.sha256,
            token,
            operation_deadline,
            on_event,
        )?;
        service_test_checkpoint(&self.state, job_id, ServiceCheckpoint::PostPublicationAudit);
        check_deadline_until_publication_commit(operation_deadline, token, OperationStage::Audit)?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Audit,
            17,
            TOTAL_PROGRESS_STEPS,
            "TXT_AUDIT_SAVED",
            "Verified publication evidence has been saved",
            false,
            on_event,
        )?;
        cleanup_renderer_workspace(&workspace, &renderer_workspace)?;
        service_test_checkpoint(&self.state, job_id, ServiceCheckpoint::Cleanup);
        check_deadline_until_publication_commit(
            operation_deadline,
            token,
            OperationStage::Cleanup,
        )?;
        self.state
            .workspaces
            .cleanup_job(job_id)
            .map_err(|_| cleanup_error())?;
        self.state
            .database()
            .clear_staging_path(job_id, Some(&normalized_path.to_string_lossy()))
            .map_err(|_| metadata_error())?;
        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Cleanup,
            TOTAL_PROGRESS_STEPS,
            TOTAL_PROGRESS_STEPS,
            "TXT_CLEANUP_COMPLETE",
            "Private renderer and staging data were cleaned up",
            false,
            on_event,
        )?;
        check_deadline_until_publication_commit(
            operation_deadline,
            token,
            OperationStage::Cleanup,
        )?;
        let completed = self.transition(
            job_id,
            JobState::Publishing,
            JobState::Completed,
            OperationStage::Cleanup,
        )?;
        emit(
            &completed,
            OperationStage::Cleanup,
            "TXT_COMPLETED",
            "The verified local TXT PDF is ready",
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
        source: &ViewerJobSource,
        verified_size: u64,
        verified_hash: &str,
        token: &CancellationToken,
        operation_deadline: Instant,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        check_deadline(operation_deadline, OperationStage::Publish)?;
        source.verify_unchanged_hash(
            verifying
                .inputs
                .first()
                .and_then(|input| input.sha256.as_deref())
                .ok_or_else(metadata_error)?,
            token,
        )?;
        check_cancelled(token, OperationStage::Publish)?;
        let state_for_reservation = self.state.clone();
        let state_for_activation = self.state.clone();
        let activation_destination = destination.to_path_buf();
        let state_for_release = self.state.clone();
        let state_for_intent = self.state.clone();
        let commit_token = token.clone();
        let result = publish_verified_staging_with_observer(
            PublicationContext {
                staging_path,
                input_paths: &[source.path.as_path()],
                destination_directory: destination,
                requested_name: &verifying.requested_output_name,
                job_id,
            },
            || token.is_cancelled() || Instant::now() >= operation_deadline,
            |_completed, _total| {
                self.progress(
                    job_id,
                    if token.commit_started() {
                        JobState::Publishing
                    } else {
                        JobState::Verifying
                    },
                    OperationStage::Publish,
                    16,
                    TOTAL_PROGRESS_STEPS,
                    "TXT_PUBLISHING",
                    "Publishing the verified PDF without overwriting an existing file",
                    !token.commit_started(),
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
                    .map_err(|_| publication_io("publication ownership could not be stored"))
            },
            move |partial, identity| {
                let proof = partial_ownership_result_code(
                    &activation_destination,
                    job_id,
                    partial,
                    identity,
                )
                .ok_or_else(|| publication_io("publication ownership proof is invalid"))?;
                state_for_activation
                    .database()
                    .activate_owned_partial(job_id, &partial.to_string_lossy(), &proof)
                    .map_err(|_| publication_io("publication ownership could not be activated"))
            },
            move |partial| {
                state_for_release
                    .database()
                    .clear_owned_partial(job_id, &partial.to_string_lossy())
                    .map_err(|_| publication_io("publication ownership could not be released"))
            },
            move |candidate| {
                if Instant::now() >= operation_deadline
                    || !commit_token.try_begin_publication_commit()
                {
                    return Err(PublicationError::Cancelled);
                }
                let resolved_name = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| publication_io("publication name is invalid"))?;
                state_for_intent
                    .database()
                    .begin_publication(
                        job_id,
                        resolved_name,
                        &candidate.to_string_lossy(),
                        verified_size,
                        verified_hash,
                    )
                    .map_err(|_| publication_io("publication intent could not be stored"))?;
                publication_commit_test_delay(job_id);
                Ok(())
            },
        )
        .map_err(|error| {
            if matches!(error, PublicationError::Cancelled)
                && !token.is_cancelled()
                && Instant::now() >= operation_deadline
            {
                operation_timeout(OperationStage::Publish)
            } else {
                publication_error(error)
            }
        })?;
        let (final_size, final_hash) =
            hash_file(&result.final_path).map_err(|_| verify_error("TXT_FINAL_REOPEN_FAILED"))?;
        if final_size != verified_size || final_hash != verified_hash {
            return Err(verify_error("TXT_FINAL_HASH_MISMATCH"));
        }
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
                .update_progress(job_id, state, stage, completed, total, ProgressUnit::Steps)
                .map_err(|_| metadata_error())?;
            database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?
        };
        emit(&job, stage, code, message, cancellable, on_event);
        Ok(())
    }

    fn current_job(&self, job_id: &str) -> Result<JobRecord, OperationError> {
        self.state
            .database()
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)
    }

    fn finish_cancelled<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        renderer_workspace: Option<&RendererWorkspace>,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        if self
            .reconcile_temporary_artifacts(job_id, workspace, renderer_workspace)
            .is_err()
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
            "TXT_CANCELLED",
            "TXT-to-PDF conversion was cancelled and private data was removed",
            false,
            on_event,
        );
        Ok(cancelled)
    }

    fn finish_failed<F>(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        renderer_workspace: Option<&RendererWorkspace>,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let current = self.current_job(job_id)?;
        let cleanup = self.reconcile_temporary_artifacts(job_id, workspace, renderer_workspace);
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        if cleanup.is_err() || current.state == JobState::Publishing {
            if cleanup.is_err() {
                database
                    .record_error_once(job_id, &cleanup_error())
                    .map_err(|_| metadata_error())?;
            }
            database
                .mark_interrupted(job_id, current.state)
                .map_err(|_| metadata_error())?;
            return Ok(());
        }
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
            "TXT_FAILED",
            "TXT-to-PDF conversion failed and private data was removed",
            false,
            on_event,
        );
        Ok(())
    }

    fn reconcile_temporary_artifacts(
        &self,
        job_id: &str,
        workspace: Option<&JobWorkspace>,
        renderer_workspace: Option<&RendererWorkspace>,
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
                    let proof =
                        partial_ownership_result_code(destination, job_id, partial_path, identity)
                            .ok_or_else(cleanup_error)?;
                    if self
                        .state
                        .database()
                        .owned_partial_is_activated(job_id, &partial_path.to_string_lossy(), &proof)
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
        if let (Some(workspace), Some(renderer_workspace)) = (workspace, renderer_workspace) {
            cleanup_renderer_workspace(workspace, renderer_workspace)?;
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
}

fn check_deadline(deadline: Instant, stage: OperationStage) -> Result<(), OperationError> {
    if Instant::now() >= deadline {
        Err(operation_timeout(stage))
    } else {
        Ok(())
    }
}

fn check_deadline_until_publication_commit(
    deadline: Instant,
    token: &CancellationToken,
    stage: OperationStage,
) -> Result<(), OperationError> {
    if token.commit_started() {
        return Ok(());
    }
    check_deadline(deadline, stage)
}

#[cfg(test)]
fn service_test_checkpoint(state: &AppState, job_id: &str, checkpoint: ServiceCheckpoint) {
    let configured = SERVICE_TEST_CANCELLATION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| value.clone());
    if configured.as_ref() == Some(&(job_id.to_owned(), checkpoint)) {
        let _ = state.cancellations.request(job_id);
    }
}

#[cfg(not(test))]
fn service_test_checkpoint(_state: &AppState, _job_id: &str, _checkpoint: ServiceCheckpoint) {}

#[cfg(test)]
fn publication_commit_test_delay(job_id: &str) {
    let configured = PUBLICATION_COMMIT_TEST_DELAY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| value.clone());
    if let Some((expected_job, delay)) = configured {
        if expected_job == job_id {
            thread::sleep(delay);
        }
    }
}

#[cfg(not(test))]
fn publication_commit_test_delay(_job_id: &str) {}

fn bounded_timeout(
    deadline: Instant,
    candidate: Duration,
    stage: OperationStage,
) -> Result<Duration, OperationError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(operation_timeout(stage));
    }
    Ok(candidate.min(remaining))
}

fn verify_qpdf_version_bounded(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    token: &CancellationToken,
    operation_deadline: Instant,
) -> Result<(), OperationError> {
    let execution = run_qpdf(
        runtime,
        workspace,
        &[OsString::from("--version")],
        token,
        bounded_timeout(
            operation_deadline,
            Duration::from_secs(30),
            OperationStage::Preflight,
        )?,
        OperationStage::Preflight,
    )?;
    check_deadline(operation_deadline, OperationStage::Preflight)?;
    if execution.exit_code != 0 || !crate::qpdf::version_output_is_expected(&execution.stdout) {
        return Err(dependency_error());
    }
    Ok(())
}

fn text_qpdf_page_count(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
    operation_deadline: Instant,
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
        bounded_timeout(
            operation_deadline,
            QPDF_VERIFY_TIMEOUT,
            OperationStage::Verify,
        )?,
        OperationStage::Verify,
    )?;
    check_deadline(operation_deadline, OperationStage::Verify)?;
    if execution.exit_code != 0 {
        return Err(verify_error("TXT_PAGE_COUNT_INVALID"));
    }
    let output = std::str::from_utf8(&execution.stdout)
        .map_err(|_| verify_error("TXT_PAGE_COUNT_INVALID"))?;
    let trimmed = output.trim();
    if trimmed.is_empty()
        || trimmed.len() > 20
        || !trimmed.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(verify_error("TXT_PAGE_COUNT_INVALID"));
    }
    trimmed
        .parse::<u64>()
        .map_err(|_| verify_error("TXT_PAGE_COUNT_INVALID"))
}

fn verify_pdf_basics(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
    operation_deadline: Instant,
) -> Result<VerifiedPdf, OperationError> {
    check_deadline(operation_deadline, OperationStage::Verify)?;
    let path = workspace.root.join(relative);
    let (canonical, _) =
        canonical_regular_file(&path).map_err(|_| verify_error("TXT_OUTPUT_NOT_REGULAR"))?;
    if canonical != path {
        return Err(verify_error("TXT_OUTPUT_PATH_INVALID"));
    }
    let metadata = fs::metadata(&canonical).map_err(|_| verify_error("TXT_OUTPUT_MISSING"))?;
    if metadata.len() == 0 || metadata.len() > TXT_MAX_PDF_BYTES {
        return Err(verify_error("TXT_OUTPUT_SIZE_LIMIT"));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .open(&canonical)
        .map_err(|_| verify_error("TXT_OUTPUT_REOPEN_FAILED"))?;
    let mut magic = [0_u8; 5];
    file.read_exact(&mut magic)
        .map_err(|_| verify_error("TXT_OUTPUT_NOT_PDF"))?;
    if &magic != b"%PDF-" {
        return Err(verify_error("TXT_OUTPUT_NOT_PDF"));
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
        bounded_timeout(
            operation_deadline,
            QPDF_VERIFY_TIMEOUT,
            OperationStage::Verify,
        )?,
        OperationStage::Verify,
    )?;
    if interpret_structural_check_exit(structural.exit_code as i32)
        != Ok(StructuralCheckOutcome::Valid)
    {
        return Err(verify_error("TXT_OUTPUT_STRUCTURE_INVALID"));
    }
    let encryption = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        bounded_timeout(
            operation_deadline,
            QPDF_VERIFY_TIMEOUT,
            OperationStage::Verify,
        )?,
        OperationStage::Verify,
    )?;
    if interpret_encryption_check_exit(encryption.exit_code as i32)
        != Ok(EncryptionCheckOutcome::Unencrypted)
    {
        return Err(verify_error("TXT_OUTPUT_ENCRYPTED"));
    }
    let pages = text_qpdf_page_count(runtime, workspace, relative, token, operation_deadline)?;
    if !(1..=TXT_MAX_PAGES).contains(&pages) {
        return Err(verify_error("TXT_PAGE_COUNT_INVALID"));
    }
    let (size, sha256) =
        hash_file(&canonical).map_err(|_| verify_error("TXT_OUTPUT_HASH_FAILED"))?;
    if size != metadata.len() {
        return Err(verify_error("TXT_OUTPUT_SIZE_CHANGED"));
    }
    Ok(VerifiedPdf {
        size,
        sha256,
        pages,
    })
}

#[allow(clippy::too_many_arguments)]
fn inspect_pdf_security(
    runtime: &crate::qpdf::VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    settings: TextToPdfSettings,
    used_scripts: &BTreeSet<AdmittedScript>,
    source: &ViewerJobSource,
    renderer_workspace: &RendererWorkspace,
    token: &CancellationToken,
    operation_deadline: Instant,
) -> Result<(), OperationError> {
    check_deadline(operation_deadline, OperationStage::Verify)?;
    let execution = run_qpdf_with_capture_limit(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--json"),
            OsString::from("--json-key=acroform"),
            OsString::from("--json-key=attachments"),
            OsString::from("--json-key=encrypt"),
            OsString::from("--json-key=outlines"),
            OsString::from("--json-key=pagelabels"),
            OsString::from("--json-key=pages"),
            OsString::from("--json-key=qpdf"),
        ],
        token,
        bounded_timeout(
            operation_deadline,
            QPDF_VERIFY_TIMEOUT,
            OperationStage::Verify,
        )?,
        OperationStage::Verify,
        QPDF_JSON_CAPTURE_LIMIT,
    )?;
    if execution.exit_code != 0 {
        return Err(verify_error("TXT_PDF_INSPECTION_FAILED"));
    }
    let document: Value = serde_json::from_slice(&execution.stdout)
        .map_err(|_| verify_error("TXT_PDF_INSPECTION_INVALID"))?;
    if document["encrypt"]["encrypted"].as_bool() != Some(false)
        || document["acroform"]["hasacroform"].as_bool() != Some(false)
        || document["attachments"]
            .as_object()
            .is_none_or(|value| !value.is_empty())
        || document["outlines"]
            .as_array()
            .is_none_or(|value| !value.is_empty())
        || document["pagelabels"]
            .as_array()
            .is_none_or(|value| !value.is_empty())
    {
        return Err(verify_error("TXT_PDF_ACTIVE_CONTENT"));
    }
    reject_forbidden_pdf_values(&document)?;
    let objects = qpdf_objects(&document)?;
    verify_page_boxes(&document, &objects, settings)?;
    verify_pdf_fonts(&objects, used_scripts)?;

    let serialized = String::from_utf8(execution.stdout)
        .map_err(|_| verify_error("TXT_PDF_INSPECTION_INVALID"))?
        .to_ascii_lowercase();
    let mut canaries = vec![
        source.path.to_string_lossy().to_ascii_lowercase(),
        source.display_name.to_ascii_lowercase(),
        workspace.root.to_string_lossy().to_ascii_lowercase(),
        renderer_workspace
            .path
            .to_string_lossy()
            .to_ascii_lowercase(),
    ];
    if let Some(user) = std::env::var_os("USERNAME") {
        canaries.push(user.to_string_lossy().to_ascii_lowercase());
    }
    if canaries
        .iter()
        .filter(|canary| !canary.is_empty())
        .any(|canary| serialized.contains(canary))
    {
        return Err(verify_error("TXT_PDF_PRIVACY_CANARY"));
    }
    Ok(())
}

fn qpdf_objects(document: &Value) -> Result<BTreeMap<String, Value>, OperationError> {
    let object_map = document["qpdf"]
        .as_array()
        .and_then(|array| array.get(1))
        .and_then(Value::as_object)
        .ok_or_else(|| verify_error("TXT_PDF_INSPECTION_INVALID"))?;
    Ok(object_map
        .iter()
        .map(|(key, value)| (key.trim_start_matches("obj:").to_owned(), value.clone()))
        .collect())
}

fn reject_forbidden_pdf_values(value: &Value) -> Result<(), OperationError> {
    const FORBIDDEN_KEYS: &[&str] = &[
        "/OpenAction",
        "/AA",
        "/JS",
        "/JavaScript",
        "/Launch",
        "/URI",
        "/GoToR",
        "/SubmitForm",
        "/ImportData",
        "/EmbeddedFiles",
        "/EF",
        "/AcroForm",
        "/XFA",
        "/Annots",
        "/Names",
        "/Metadata",
        "/Info",
        "/CreationDate",
        "/ModDate",
        "/Producer",
        "/Creator",
        "/Title",
        "/Author",
        "/Subject",
        "/Keywords",
        "/FFilter",
        "/FDecodeParms",
    ];
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN_KEYS.contains(&key.as_str())
                    || (key == "/F" && map.contains_key("/Length"))
                    || child.as_str() == Some("/Filespec")
                {
                    return Err(verify_error("TXT_PDF_ACTIVE_CONTENT"));
                }
                reject_forbidden_pdf_values(child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                reject_forbidden_pdf_values(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn verify_page_boxes(
    document: &Value,
    objects: &BTreeMap<String, Value>,
    settings: TextToPdfSettings,
) -> Result<(), OperationError> {
    let pages = document["pages"]
        .as_array()
        .ok_or_else(|| verify_error("TXT_PDF_PAGES_INVALID"))?;
    let (width, height) = settings.paper_inches();
    let expected = [0.0, 0.0, width * 72.0, height * 72.0];
    for page in pages {
        let reference = page["object"]
            .as_str()
            .ok_or_else(|| verify_error("TXT_PDF_PAGES_INVALID"))?;
        let value = objects
            .get(reference)
            .and_then(|object| object["value"].as_object())
            .ok_or_else(|| verify_error("TXT_PDF_PAGES_INVALID"))?;
        let media = pdf_box(value.get("/MediaBox"))?;
        if media
            .iter()
            .zip(expected.iter())
            .any(|(actual, expected)| (actual - expected).abs() > 0.5)
        {
            return Err(verify_error("TXT_PDF_MEDIA_BOX"));
        }
        if let Some(crop) = value.get("/CropBox") {
            let crop = pdf_box(Some(crop))?;
            if crop
                .iter()
                .zip(media.iter())
                .any(|(crop, media)| (crop - media).abs() > f64::EPSILON)
            {
                return Err(verify_error("TXT_PDF_CROP_BOX"));
            }
        }
        if value.get("/Rotate").and_then(Value::as_i64).unwrap_or(0) != 0 {
            return Err(verify_error("TXT_PDF_ROTATE"));
        }
    }
    Ok(())
}

fn pdf_box(value: Option<&Value>) -> Result<[f64; 4], OperationError> {
    let values = value
        .and_then(Value::as_array)
        .filter(|values| values.len() == 4)
        .ok_or_else(|| verify_error("TXT_PDF_BOX_INVALID"))?;
    let mut result = [0.0; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value
            .as_f64()
            .ok_or_else(|| verify_error("TXT_PDF_BOX_INVALID"))?;
    }
    Ok(result)
}

fn verify_pdf_fonts(
    objects: &BTreeMap<String, Value>,
    used_scripts: &BTreeSet<AdmittedScript>,
) -> Result<(), OperationError> {
    let allowed = [
        "NotoSans-Regular",
        "NotoSansDevanagari-Regular",
        "NotoSansTelugu-Regular",
    ];
    let mut seen = BTreeSet::new();
    for object in objects.values() {
        let Some(value) = object.get("value").and_then(Value::as_object) else {
            continue;
        };
        let Some(base_font) = value.get("/BaseFont").and_then(Value::as_str) else {
            continue;
        };
        let base_font = normalize_pdf_font_name(base_font.trim_start_matches('/'));
        if !allowed.contains(&base_font)
            || base_font.contains("Bold")
            || base_font.contains("Italic")
        {
            return Err(verify_error("TXT_PDF_FONT_INVENTORY"));
        }
        seen.insert(base_font.to_owned());
        if !font_has_file2(value, objects)? {
            return Err(verify_error("TXT_PDF_FONT_NOT_EMBEDDED"));
        }
    }
    for expected in used_scripts.iter().map(|script| match script {
        AdmittedScript::LatinCommon => "NotoSans-Regular",
        AdmittedScript::Devanagari => "NotoSansDevanagari-Regular",
        AdmittedScript::Telugu => "NotoSansTelugu-Regular",
    }) {
        if !seen.contains(expected) {
            return Err(verify_error("TXT_PDF_FONT_MISSING"));
        }
    }
    Ok(())
}

fn normalize_pdf_font_name(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() > 7
        && bytes[..6].iter().all(u8::is_ascii_uppercase)
        && bytes.get(6) == Some(&b'+')
    {
        &name[7..]
    } else {
        name
    }
}

fn font_has_file2(
    font: &serde_json::Map<String, Value>,
    objects: &BTreeMap<String, Value>,
) -> Result<bool, OperationError> {
    if let Some(reference) = font.get("/FontDescriptor").and_then(Value::as_str) {
        return descriptor_has_file2(reference, objects);
    }
    if let Some(descendants) = font.get("/DescendantFonts").and_then(Value::as_array) {
        for descendant in descendants {
            let reference = descendant
                .as_str()
                .ok_or_else(|| verify_error("TXT_PDF_FONT_INVENTORY"))?;
            let descendant = objects
                .get(reference)
                .and_then(|object| object["value"].as_object())
                .ok_or_else(|| verify_error("TXT_PDF_FONT_INVENTORY"))?;
            if let Some(descriptor) = descendant.get("/FontDescriptor").and_then(Value::as_str) {
                if descriptor_has_file2(descriptor, objects)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn descriptor_has_file2(
    reference: &str,
    objects: &BTreeMap<String, Value>,
) -> Result<bool, OperationError> {
    let descriptor = objects
        .get(reference)
        .and_then(|object| object["value"].as_object())
        .ok_or_else(|| verify_error("TXT_PDF_FONT_INVENTORY"))?;
    if descriptor.contains_key("/FontFile") || descriptor.contains_key("/FontFile3") {
        return Ok(false);
    }
    let Some(file2) = descriptor.get("/FontFile2").and_then(Value::as_str) else {
        return Ok(false);
    };
    Ok(objects
        .get(file2)
        .is_some_and(|object| object.get("stream").is_some()))
}

fn create_renderer_workspace(
    workspace: &JobWorkspace,
    job_id: &str,
) -> Result<RendererWorkspace, OperationError> {
    let generation = Uuid::new_v4().hyphenated().to_string();
    let parent = workspace.temporary.join("renderer-udfs");
    fs::create_dir(&parent).map_err(|_| workspace_error())?;
    let path = parent.join(&generation);
    fs::create_dir(&path).map_err(|_| workspace_error())?;
    reject_reparse_components(&path).map_err(|_| workspace_error())?;
    let marker_value = format!("job={job_id}\ngeneration={generation}\n");
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join(UDF_MARKER))
        .map_err(|_| workspace_error())?;
    marker
        .write_all(marker_value.as_bytes())
        .and_then(|_| marker.sync_all())
        .map_err(|_| workspace_error())?;
    Ok(RendererWorkspace {
        path,
        marker_value,
        generation,
    })
}

fn cleanup_renderer_workspace(
    workspace: &JobWorkspace,
    renderer: &RendererWorkspace,
) -> Result<(), OperationError> {
    if !renderer.path.exists() {
        return Ok(());
    }
    reject_reparse_components(&renderer.path).map_err(|_| cleanup_error())?;
    let expected_parent = workspace.temporary.join("renderer-udfs");
    if renderer.path.parent() != Some(expected_parent.as_path())
        || renderer.path.file_name() != Some(renderer.generation.as_ref())
    {
        return Err(cleanup_error());
    }
    let marker = fs::read_to_string(renderer.path.join(UDF_MARKER)).map_err(|_| cleanup_error())?;
    if marker != renderer.marker_value {
        return Err(cleanup_error());
    }
    for attempt in 0..=20 {
        match fs::remove_dir_all(&renderer.path) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 20 => thread::sleep(Duration::from_millis(100)),
            Err(_) => return Err(cleanup_error()),
        }
    }
    Err(cleanup_error())
}

pub fn validate_recovery_renderer_workspaces(
    workspace_root: &Path,
    job_id: &str,
) -> Result<(), OperationError> {
    let job_id = Uuid::parse_str(job_id)
        .map_err(|_| cleanup_error())?
        .hyphenated()
        .to_string();
    if workspace_root.file_name() != Some(job_id.as_ref()) {
        return Err(cleanup_error());
    }
    let parent = workspace_root.join("temp").join("renderer-udfs");
    if !parent.exists() {
        return Ok(());
    }
    reject_reparse_components(&parent).map_err(|_| cleanup_error())?;
    for entry in fs::read_dir(&parent).map_err(|_| cleanup_error())? {
        let entry = entry.map_err(|_| cleanup_error())?;
        let file_type = entry.file_type().map_err(|_| cleanup_error())?;
        if !file_type.is_dir() || file_type.is_symlink() {
            return Err(cleanup_error());
        }
        let generation = entry
            .file_name()
            .to_str()
            .ok_or_else(cleanup_error)
            .and_then(|value| {
                Uuid::parse_str(value)
                    .map_err(|_| cleanup_error())
                    .map(|uuid| uuid.hyphenated().to_string())
            })?;
        if entry.file_name() != generation.as_str() {
            return Err(cleanup_error());
        }
        reject_reparse_components(&entry.path()).map_err(|_| cleanup_error())?;
        let marker =
            fs::read_to_string(entry.path().join(UDF_MARKER)).map_err(|_| cleanup_error())?;
        if marker != format!("job={job_id}\ngeneration={generation}\n") {
            return Err(cleanup_error());
        }
        prove_exclusive_cleanup_access(&entry.path())?;
    }
    Ok(())
}

fn prove_exclusive_cleanup_access(path: &Path) -> Result<(), OperationError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| cleanup_error())?;
    if metadata.file_type().is_symlink() {
        return Err(cleanup_error());
    }
    let mut options = OpenOptions::new();
    options
        .access_mode(FILE_READ_ATTRIBUTES | DELETE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if metadata.is_dir() {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    } else if !metadata.is_file() {
        return Err(cleanup_error());
    }
    let handle = options.open(path).map_err(|_| cleanup_error())?;
    drop(handle);
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| cleanup_error())? {
            prove_exclusive_cleanup_access(&entry.map_err(|_| cleanup_error())?.path())?;
        }
    }
    Ok(())
}

fn write_private_audit(
    workspace: &JobWorkspace,
    settings: TextToPdfSettings,
    render: &TextRenderEvidence,
    source_hash: &str,
    raw: &VerifiedPdf,
    normalized: &VerifiedPdf,
) -> Result<(), OperationError> {
    let value = json!({
        "schemaVersion": 1,
        "operation": "text.to-pdf@1.0.0",
        "settings": settings,
        "rendererGeneration": render.renderer_generation,
        "webView2Runtime": render.runtime_version,
        "servedUrls": render.served_urls,
        "deniedRequests": render.denied_requests,
        "sourceSha256": source_hash,
        "raw": {"bytes": raw.size, "sha256": raw.sha256, "pages": raw.pages},
        "normalized": {"bytes": normalized.size, "sha256": normalized.sha256, "pages": normalized.pages}
    });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| metadata_error())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(workspace.audit.join("text-to-pdf-evidence.json"))
        .map_err(|_| workspace_error())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| workspace_error())
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
        Err(OperationError::safe(
            "CANCELLED",
            "TXT-to-PDF conversion was cancelled",
            "Private renderer and staging data will be removed safely.",
            stage,
            false,
        ))
    } else {
        Ok(())
    }
}

fn operation_timeout(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "TXT_OPERATION_TIMEOUT",
        "TXT-to-PDF conversion exceeded its bounded runtime",
        "No unverified output was published; retry after local resources are available.",
        stage,
        true,
    )
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn publication_io(message: &str) -> PublicationError {
    PublicationError::Io(std::io::Error::other(message))
}

fn publication_error(error: PublicationError) -> OperationError {
    match error {
        PublicationError::Cancelled => OperationError::safe(
            "CANCELLED",
            "TXT-to-PDF conversion was cancelled",
            "No output was published.",
            OperationStage::Publish,
            false,
        ),
        _ => OperationError::safe(
            "TXT_PUBLICATION_FAILED",
            "The verified PDF could not be published",
            "No existing destination file was overwritten.",
            OperationStage::Publish,
            true,
        ),
    }
}

fn invalid_request() -> OperationError {
    OperationError::safe(
        "INVALID_REQUEST",
        "The TXT-to-PDF request is invalid",
        "Select one local TXT input and the fixed supported settings.",
        OperationStage::Inspect,
        false,
    )
}

fn invalid_output_name() -> OperationError {
    OperationError::safe(
        "INVALID_OUTPUT_NAME",
        "Choose a PDF output name",
        "The output must use a safe local .pdf filename.",
        OperationStage::Inspect,
        false,
    )
}

fn size_limit() -> OperationError {
    OperationError::safe(
        "TXT_SIZE_LIMIT",
        "The TXT file is too large",
        "TXT input is limited to 8,388,608 bytes.",
        OperationStage::Preflight,
        false,
    )
}

fn path_error() -> OperationError {
    OperationError::safe(
        "PATH_UNSAFE",
        "The local path is not safe",
        "Choose a regular local TXT and an existing local destination.",
        OperationStage::Inspect,
        false,
    )
}

fn source_changed(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "SOURCE_CHANGED",
        "The source TXT changed",
        "Select the source again before converting.",
        stage,
        true,
    )
}

fn dependency_error() -> OperationError {
    OperationError::safe(
        "TXT_DEPENDENCY_UNAVAILABLE",
        "A local TXT-to-PDF dependency is unavailable",
        "No output was published. Repair the local runtime and try again.",
        OperationStage::Preflight,
        true,
    )
}

fn workspace_error() -> OperationError {
    OperationError::safe(
        "TXT_WORKSPACE_FAILED",
        "The private TXT workspace could not be prepared",
        "No user output was created.",
        OperationStage::Plan,
        true,
    )
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "TXT_CLEANUP_RETRY_REQUIRED",
        "Private TXT cleanup needs attention",
        "The published file was preserved; retry cleanup after the owned renderer releases its files.",
        OperationStage::Cleanup,
        true,
    )
}

fn verify_error(code: &str) -> OperationError {
    OperationError::safe(
        code,
        "The TXT PDF failed verification",
        "No unverified output was published.",
        OperationStage::Verify,
        false,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "TXT_METADATA_FAILED",
        "The TXT job metadata is unavailable",
        "No output was published.",
        OperationStage::Audit,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_text_test_job(state: &AppState, lane: &Path, label: &str) -> JobRecord {
        let source = lane.join(format!("{label}.txt"));
        let destination = lane.join(format!("{label}-destination"));
        fs::create_dir_all(&destination).unwrap();
        fs::write(&source, format!("private-{label}-content")).unwrap();
        let input = state.viewer_sessions.open_txt(&source).unwrap();
        let grant = state
            .viewer_sessions
            .grant_destination(&destination)
            .unwrap();
        TextToPdfService::new(state.clone())
            .create_job(crate::contracts::TextToPdfJobCreateRequest {
                operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                input_session_id: input.session_id,
                input_generation: input.generation,
                destination_grant_id: grant.grant_id,
                requested_output_name: format!("{label}.pdf"),
                settings: TextToPdfSettings {
                    page_size: crate::text_to_pdf::TextPageSize::A4,
                    orientation: crate::text_to_pdf::TextOrientation::Portrait,
                },
            })
            .unwrap()
    }

    fn advance_text_job(state: &AppState, job_id: &str, target: JobState) {
        let mut current = JobState::Queued;
        for (next, stage) in [
            (JobState::Inspecting, OperationStage::Inspect),
            (JobState::Preflight, OperationStage::Preflight),
            (JobState::Ready, OperationStage::Plan),
            (JobState::Running, OperationStage::Execute),
            (JobState::Verifying, OperationStage::Verify),
            (JobState::Publishing, OperationStage::Publish),
        ] {
            if current == target {
                return;
            }
            let mut database = state.database();
            let version = database.get_job(job_id).unwrap().unwrap().version;
            database
                .transition_job(job_id, current, version, next, Some(stage))
                .unwrap();
            current = next;
        }
        if target == JobState::Interrupted {
            state
                .database()
                .mark_interrupted(job_id, JobState::Publishing)
                .unwrap();
            return;
        }
        assert_eq!(current, target);
    }

    #[test]
    fn subset_prefix_normalization_is_exact() {
        assert_eq!(
            normalize_pdf_font_name("ABCDEF+NotoSans-Regular"),
            "NotoSans-Regular"
        );
        assert_eq!(
            normalize_pdf_font_name("ABCDE+NotoSans-Regular"),
            "ABCDE+NotoSans-Regular"
        );
        assert_eq!(
            normalize_pdf_font_name("abcdef+NotoSans-Regular"),
            "abcdef+NotoSans-Regular"
        );
    }

    #[test]
    fn normalization_arguments_are_page_only_and_exact() {
        let args = build_text_pdf_normalization_arguments(
            Path::new(TEXT_RAW_STAGING_RELATIVE_PATH),
            Path::new(TEXT_NORMALIZED_STAGING_RELATIVE_PATH),
        )
        .expect("arguments");
        assert_eq!(args[0], "--empty");
        assert!(args.contains(&OsString::from("--remove-info")));
        assert!(args.contains(&OsString::from("--remove-metadata")));
        assert!(args.contains(&OsString::from("--remove-page-labels")));
        assert!(!args.iter().any(|value| value == "--deterministic-id"));
    }

    #[test]
    fn cancellation_is_typed_at_every_lifecycle_stage() {
        for stage in [
            OperationStage::Inspect,
            OperationStage::Preflight,
            OperationStage::Plan,
            OperationStage::Execute,
            OperationStage::Verify,
            OperationStage::Publish,
            OperationStage::Audit,
            OperationStage::Cleanup,
            OperationStage::Recovery,
        ] {
            let registry = crate::app_state::CancellationRegistry::default();
            let token = registry.register("job");
            registry.request("job");
            let error = check_cancelled(&token, stage).unwrap_err();
            assert_eq!(error.code, "CANCELLED");
            assert_eq!(error.stage, stage);
        }
    }

    #[test]
    fn total_operation_deadline_bounds_every_candidate_timeout() {
        let deadline = Instant::now() + Duration::from_millis(100);
        let bounded =
            bounded_timeout(deadline, QPDF_NORMALIZE_TIMEOUT, OperationStage::Verify).unwrap();
        assert!(bounded <= Duration::from_millis(100));
        let expired = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            bounded_timeout(expired, QPDF_VERIFY_TIMEOUT, OperationStage::Verify)
                .unwrap_err()
                .code,
            "TXT_OPERATION_TIMEOUT"
        );
    }

    #[test]
    fn deadline_crossing_after_publication_commit_records_truthful_terminal_evidence() {
        let lane = tempfile::tempdir().unwrap();
        let app_data = lane.path().join("app-data");
        let destination = lane.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let input_path = lane.path().join("deadline.txt");
        fs::write(&input_path, "publication deadline boundary").unwrap();
        let state = crate::initialize_runtime(&app_data, Utc::now()).unwrap();
        let input = state.viewer_sessions.open_txt(&input_path).unwrap();
        let grant = state
            .viewer_sessions
            .grant_destination(&destination)
            .unwrap();
        let source = state
            .viewer_sessions
            .source_for_text_job(&input.session_id, input.generation)
            .unwrap();
        let service = TextToPdfService::new(state.clone());
        let created = service
            .create_job(crate::contracts::TextToPdfJobCreateRequest {
                operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                input_session_id: input.session_id,
                input_generation: input.generation,
                destination_grant_id: grant.grant_id,
                requested_output_name: "deadline.pdf".to_owned(),
                settings: TextToPdfSettings {
                    page_size: crate::text_to_pdf::TextPageSize::A4,
                    orientation: crate::text_to_pdf::TextOrientation::Portrait,
                },
            })
            .unwrap();
        let token = state.cancellations.register(&created.id);
        let (_, source_hash) = source.read_all_bounded(TXT_MAX_RAW_BYTES, &token).unwrap();
        state
            .database()
            .update_input_hash(&created.id, 0, &source_hash)
            .unwrap();
        advance_text_job(&state, &created.id, JobState::Verifying);
        let verifying = state.database().get_job(&created.id).unwrap().unwrap();
        let staging = lane.path().join("verified.pdf");
        fs::write(&staging, b"verified-private-pdf-bytes").unwrap();
        let (verified_size, verified_hash) = hash_file(&staging).unwrap();
        state
            .database()
            .set_output_staging(
                &created.id,
                &staging.to_string_lossy(),
                verified_size,
                &verified_hash,
                &timestamp(),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        *PUBLICATION_COMMIT_TEST_DELAY
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some((created.id.clone(), Duration::from_millis(2_200)));
        service
            .publish(
                &created.id,
                &verifying,
                &staging,
                &destination,
                &source,
                verified_size,
                &verified_hash,
                &token,
                deadline,
                &mut |_| {},
            )
            .unwrap();
        *PUBLICATION_COMMIT_TEST_DELAY.get().unwrap().lock().unwrap() = None;

        assert!(Instant::now() >= deadline);
        assert!(token.commit_started());
        check_deadline_until_publication_commit(deadline, &token, OperationStage::Audit).unwrap();
        check_deadline_until_publication_commit(deadline, &token, OperationStage::Cleanup).unwrap();
        let published = state.database().get_job(&created.id).unwrap().unwrap();
        let output = &published.outputs[0];
        assert_eq!(output.status, OutputStatus::Published);
        let final_path = Path::new(output.final_path.as_deref().unwrap());
        assert_eq!(
            hash_file(final_path).unwrap(),
            (verified_size, verified_hash)
        );
        state
            .database()
            .clear_staging_path(&created.id, Some(&staging.to_string_lossy()))
            .unwrap();
        let completed = service
            .transition(
                &created.id,
                JobState::Publishing,
                JobState::Completed,
                OperationStage::Cleanup,
            )
            .unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert!(final_path.exists());
    }

    #[test]
    fn pre_cancelled_job_publishes_nothing_and_reconciles_terminal_state() {
        let lane = tempfile::tempdir().unwrap();
        let app_data = lane.path().join("app-data");
        let destination = lane.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let input_path = lane.path().join("cancel.txt");
        fs::write(&input_path, "cancel before renderer creation").unwrap();
        let state = crate::initialize_runtime(&app_data, Utc::now()).unwrap();
        let input = state.viewer_sessions.open_txt(&input_path).unwrap();
        let grant = state
            .viewer_sessions
            .grant_destination(&destination)
            .unwrap();
        let source = state
            .viewer_sessions
            .source_for_text_job(&input.session_id, input.generation)
            .unwrap();
        let service = TextToPdfService::new(state.clone());
        let created = service
            .create_job(crate::contracts::TextToPdfJobCreateRequest {
                operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                input_session_id: input.session_id,
                input_generation: input.generation,
                destination_grant_id: grant.grant_id,
                requested_output_name: "cancelled.pdf".to_owned(),
                settings: TextToPdfSettings {
                    page_size: crate::text_to_pdf::TextPageSize::A4,
                    orientation: crate::text_to_pdf::TextOrientation::Portrait,
                },
            })
            .unwrap();
        let token = state.cancellations.register(&created.id);
        assert_eq!(
            state.cancellations.request(&created.id),
            crate::app_state::CancelOutcome::Requested
        );
        let cancelled = service
            .execute_with_registered_token(&created.id, source, token, |_| {})
            .unwrap();
        assert_eq!(cancelled.state, JobState::Cancelled);
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert!(cancelled.outputs[0].final_path.is_none());
    }

    #[test]
    fn txt_job_metadata_never_persists_source_paths_or_private_text() {
        let lane = tempfile::tempdir().unwrap();
        let state = crate::initialize_runtime(&lane.path().join("app-data"), Utc::now()).unwrap();
        let canary = "txt-source-path-canary-94b18c";
        let job = create_text_test_job(&state, lane.path(), canary);
        let persisted = state.database().get_job(&job.id).unwrap().unwrap();
        assert!(persisted.inputs[0].source_path.is_empty());
        assert!(persisted.inputs[0].canonical_path.is_empty());
        let database = state.database();
        let leaked: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM job_inputs
                 WHERE source_path LIKE ?1 OR canonical_path LIKE ?1",
                [format!("%{canary}%")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leaked, 0);
    }

    #[test]
    fn txt_recovery_marks_every_unpublished_nonterminal_state_interrupted() {
        for target in [
            JobState::Queued,
            JobState::Inspecting,
            JobState::Preflight,
            JobState::Ready,
            JobState::Running,
            JobState::Verifying,
            JobState::Publishing,
            JobState::Interrupted,
        ] {
            let lane = tempfile::tempdir().unwrap();
            let state =
                crate::initialize_runtime(&lane.path().join("app-data"), Utc::now()).unwrap();
            let job = create_text_test_job(&state, lane.path(), &format!("state-{target:?}"));
            advance_text_job(&state, &job.id, target);
            let workspace = state.workspaces.create_job(&job.id).unwrap();
            fs::write(workspace.staging.join("abandoned.pdf"), b"private").unwrap();

            let report = crate::recovery::reconcile_startup(&state).unwrap();
            let recovered = state.database().get_job(&job.id).unwrap().unwrap();
            assert_eq!(recovered.state, JobState::Interrupted, "{target:?}");
            assert_eq!(report.failed, 0, "{target:?}");
            assert_eq!(report.interrupted, 1, "{target:?}");
            assert!(!workspace.root.exists(), "{target:?}");
        }
    }

    #[test]
    fn txt_recovery_preserves_locked_udf_and_retries_only_after_release() {
        let lane = tempfile::tempdir().unwrap();
        let state = crate::initialize_runtime(&lane.path().join("app-data"), Utc::now()).unwrap();
        let job = create_text_test_job(&state, lane.path(), "locked-udf");
        advance_text_job(&state, &job.id, JobState::Running);
        let workspace = state.workspaces.create_job(&job.id).unwrap();
        let generation = Uuid::new_v4().hyphenated().to_string();
        let udf = workspace.temporary.join("renderer-udfs").join(&generation);
        fs::create_dir_all(&udf).unwrap();
        fs::write(
            udf.join(UDF_MARKER),
            format!("job={}\ngeneration={generation}\n", job.id),
        )
        .unwrap();
        let locked_path = udf.join("owned.lock");
        fs::write(&locked_path, b"owned").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(
                windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                    | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
            )
            .open(&locked_path)
            .unwrap();

        let first = crate::recovery::reconcile_startup(&state).unwrap();
        assert_eq!(first.cleanup_failures, 1);
        assert_eq!(
            state.database().get_job(&job.id).unwrap().unwrap().state,
            JobState::Interrupted
        );
        assert!(workspace.root.exists());

        drop(locked);
        assert!(
            validate_recovery_renderer_workspaces(&workspace.root, &job.id).is_ok(),
            "released UDF must pass the exclusive ownership probe"
        );
        let recovered = crate::recovery::resolve_interrupted(&state, &job.id).unwrap();
        assert_eq!(recovered.state, JobState::Interrupted);
        assert!(!workspace.root.exists());
    }

    #[test]
    fn recovery_accepts_only_exact_job_generation_udf_markers() {
        let directory = tempfile::tempdir().unwrap();
        let job_id = Uuid::new_v4().hyphenated().to_string();
        let generation = Uuid::new_v4().hyphenated().to_string();
        let root = directory.path().join(&job_id);
        let udf = root.join("temp").join("renderer-udfs").join(&generation);
        fs::create_dir_all(&udf).unwrap();
        fs::write(
            udf.join(UDF_MARKER),
            format!("job={job_id}\ngeneration={generation}\n"),
        )
        .unwrap();
        assert!(validate_recovery_renderer_workspaces(&root, &job_id).is_ok());
        fs::write(udf.join(UDF_MARKER), "wrong").unwrap();
        assert!(validate_recovery_renderer_workspaces(&root, &job_id).is_err());
        fs::write(
            udf.join(UDF_MARKER),
            format!("job={job_id}\ngeneration={generation}\n"),
        )
        .unwrap();
        fs::write(
            root.join("temp").join("renderer-udfs").join("unknown"),
            b"x",
        )
        .unwrap();
        assert!(validate_recovery_renderer_workspaces(&root, &job_id).is_err());
    }

    #[test]
    fn pdf_page_boxes_cover_all_exact_settings_and_reject_crop_or_rotation() {
        for settings in [
            TextToPdfSettings {
                page_size: crate::text_to_pdf::TextPageSize::A4,
                orientation: crate::text_to_pdf::TextOrientation::Portrait,
            },
            TextToPdfSettings {
                page_size: crate::text_to_pdf::TextPageSize::A4,
                orientation: crate::text_to_pdf::TextOrientation::Landscape,
            },
            TextToPdfSettings {
                page_size: crate::text_to_pdf::TextPageSize::Letter,
                orientation: crate::text_to_pdf::TextOrientation::Portrait,
            },
            TextToPdfSettings {
                page_size: crate::text_to_pdf::TextPageSize::Letter,
                orientation: crate::text_to_pdf::TextOrientation::Landscape,
            },
        ] {
            let (width, height) = settings.paper_inches();
            let document = json!({"pages": [{"object": "1 0 R"}]});
            let mut objects = BTreeMap::from([(
                "1 0 R".to_owned(),
                json!({"value": {"/MediaBox": [0.0, 0.0, width * 72.0, height * 72.0], "/Rotate": 0}}),
            )]);
            assert!(verify_page_boxes(&document, &objects, settings).is_ok());
            objects.get_mut("1 0 R").unwrap()["value"]["/CropBox"] = json!([0.0, 0.0, 10.0, 10.0]);
            assert_eq!(
                verify_page_boxes(&document, &objects, settings)
                    .unwrap_err()
                    .code,
                "TXT_PDF_CROP_BOX"
            );
            objects.get_mut("1 0 R").unwrap()["value"]
                .as_object_mut()
                .unwrap()
                .remove("/CropBox");
            objects.get_mut("1 0 R").unwrap()["value"]["/Rotate"] = json!(90);
            assert_eq!(
                verify_page_boxes(&document, &objects, settings)
                    .unwrap_err()
                    .code,
                "TXT_PDF_ROTATE"
            );
        }
    }

    #[test]
    fn pdf_active_content_and_font_inventory_fail_closed() {
        for key in [
            "/OpenAction",
            "/AA",
            "/JS",
            "/JavaScript",
            "/Launch",
            "/URI",
            "/GoToR",
            "/SubmitForm",
            "/ImportData",
            "/EmbeddedFiles",
            "/EF",
            "/AcroForm",
            "/XFA",
            "/Annots",
            "/Names",
            "/Metadata",
            "/Info",
            "/CreationDate",
            "/ModDate",
            "/Producer",
            "/Creator",
            "/Title",
            "/Author",
            "/Subject",
            "/Keywords",
            "/FFilter",
            "/FDecodeParms",
        ] {
            let candidate = Value::Object(serde_json::Map::from_iter([(
                key.to_owned(),
                Value::Bool(true),
            )]));
            assert_eq!(
                reject_forbidden_pdf_values(&candidate).unwrap_err().code,
                "TXT_PDF_ACTIVE_CONTENT",
                "{key}"
            );
        }

        let mut objects = BTreeMap::from([
            (
                "1 0 R".to_owned(),
                json!({"value": {"/BaseFont": "/ABCDEF+NotoSans-Regular", "/FontDescriptor": "2 0 R"}}),
            ),
            (
                "2 0 R".to_owned(),
                json!({"value": {"/FontFile2": "3 0 R"}}),
            ),
            (
                "3 0 R".to_owned(),
                json!({"stream": {"dict": {"/Length": 7}}}),
            ),
        ]);
        assert!(verify_pdf_fonts(&objects, &BTreeSet::from([AdmittedScript::LatinCommon])).is_ok());
        objects.get_mut("1 0 R").unwrap()["value"]["/BaseFont"] = json!("/ABCDEF+ArialMT");
        assert_eq!(
            verify_pdf_fonts(&objects, &BTreeSet::from([AdmittedScript::LatinCommon]))
                .unwrap_err()
                .code,
            "TXT_PDF_FONT_INVENTORY"
        );
        objects.get_mut("1 0 R").unwrap()["value"]["/BaseFont"] = json!("/ABCDEF+NotoSans-Regular");
        objects.get_mut("2 0 R").unwrap()["value"]
            .as_object_mut()
            .unwrap()
            .remove("/FontFile2");
        assert_eq!(
            verify_pdf_fonts(&objects, &BTreeSet::from([AdmittedScript::LatinCommon]))
                .unwrap_err()
                .code,
            "TXT_PDF_FONT_NOT_EMBEDDED"
        );
    }

    #[test]
    #[ignore = "runs the real hidden WebView2 and accepted qpdf AppContainer"]
    fn native_webview2_qpdf_acceptance_covers_all_page_settings_and_mixed_scripts() {
        crate::webview2_environment::enforce_webview2_environment_policy();
        let lane = tempfile::tempdir().unwrap();
        let app_data = lane.path().join("app-data");
        let destination = lane.path().join("published");
        fs::create_dir(&destination).unwrap();
        let input_path = lane.path().join("mixed.txt");
        fs::write(
            &input_path,
            "English <script>alert('literal')</script>\nहिन्दी क्\u{200d}ष\nతెలుగు క్\u{200c}ష\n",
        )
        .unwrap();
        let state = crate::initialize_runtime_with_resources(
            &app_data,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Utc::now(),
        )
        .unwrap();
        let input = state.viewer_sessions.open_txt(&input_path).unwrap();
        let grant = state
            .viewer_sessions
            .grant_destination(&destination)
            .unwrap();

        for (index, page_size, orientation) in [
            (
                0,
                crate::text_to_pdf::TextPageSize::A4,
                crate::text_to_pdf::TextOrientation::Portrait,
            ),
            (
                1,
                crate::text_to_pdf::TextPageSize::A4,
                crate::text_to_pdf::TextOrientation::Landscape,
            ),
            (
                2,
                crate::text_to_pdf::TextPageSize::Letter,
                crate::text_to_pdf::TextOrientation::Portrait,
            ),
            (
                3,
                crate::text_to_pdf::TextPageSize::Letter,
                crate::text_to_pdf::TextOrientation::Landscape,
            ),
        ] {
            let source = state
                .viewer_sessions
                .source_for_text_job(&input.session_id, input.generation)
                .unwrap();
            let service = TextToPdfService::new(state.clone());
            let created = service
                .create_job(crate::contracts::TextToPdfJobCreateRequest {
                    operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                    input_session_id: input.session_id.clone(),
                    input_generation: input.generation,
                    destination_grant_id: grant.grant_id.clone(),
                    requested_output_name: format!("mixed-{index}.pdf"),
                    settings: TextToPdfSettings {
                        page_size,
                        orientation,
                    },
                })
                .unwrap();
            let token = state.cancellations.register(&created.id);
            let completed = service
                .execute_with_registered_token(&created.id, source, token, |_| {})
                .unwrap_or_else(|error| panic!("{}: {}", error.code, error.detail));
            assert_eq!(completed.state, JobState::Completed);
            let output = completed.outputs[0].final_path.as_ref().unwrap();
            let bytes = fs::read(output).unwrap();
            assert!(bytes.starts_with(b"%PDF-"));
            assert!(bytes.len() > 1_000);
            if index == 0 {
                if let Some(evidence_directory) =
                    std::env::var_os("DOCUMENT_STUDIO_G04E1_VISUAL_EVIDENCE_DIR")
                {
                    let evidence_directory = PathBuf::from(evidence_directory);
                    fs::create_dir_all(&evidence_directory).unwrap();
                    fs::copy(output, evidence_directory.join("mixed-a4-portrait.pdf")).unwrap();
                }
            }
        }
        state
            .viewer_sessions
            .close(&crate::contracts::ViewerSessionRequest {
                session_id: input.session_id,
                generation: input.generation,
            })
            .unwrap();
        for (name, bytes) in [
            ("empty", b"".as_slice()),
            ("whitespace", b" \t\n".as_slice()),
        ] {
            let path = lane.path().join(format!("{name}.txt"));
            fs::write(&path, bytes).unwrap();
            let input = state.viewer_sessions.open_txt(&path).unwrap();
            let source = state
                .viewer_sessions
                .source_for_text_job(&input.session_id, input.generation)
                .unwrap();
            let service = TextToPdfService::new(state.clone());
            let created = service
                .create_job(crate::contracts::TextToPdfJobCreateRequest {
                    operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                    input_session_id: input.session_id.clone(),
                    input_generation: input.generation,
                    destination_grant_id: grant.grant_id.clone(),
                    requested_output_name: format!("{name}.pdf"),
                    settings: TextToPdfSettings {
                        page_size: crate::text_to_pdf::TextPageSize::A4,
                        orientation: crate::text_to_pdf::TextOrientation::Portrait,
                    },
                })
                .unwrap();
            let token = state.cancellations.register(&created.id);
            let completed = service
                .execute_with_registered_token(&created.id, source, token, |_| {})
                .unwrap_or_else(|error| panic!("{}: {}", error.code, error.detail));
            assert_eq!(completed.state, JobState::Completed);
            let output = completed.outputs[0].final_path.as_ref().unwrap();
            assert!(fs::read(output).unwrap().starts_with(b"%PDF-"));
            state
                .viewer_sessions
                .close(&crate::contracts::ViewerSessionRequest {
                    session_id: input.session_id,
                    generation: input.generation,
                })
                .unwrap();
        }
        state.viewer_sessions.close_all();
    }

    #[test]
    #[ignore = "runs real WebView2/qpdf cancellation at service-owned checkpoints"]
    fn native_text_service_cancellation_matrix_cleans_owned_state_and_preserves_commits() {
        crate::webview2_environment::enforce_webview2_environment_policy();
        let lane = tempfile::tempdir().unwrap();
        let app_data = lane.path().join("app-data");
        let destination = lane.path().join("published");
        fs::create_dir(&destination).unwrap();
        let input_path = lane.path().join("cancellation.txt");
        fs::write(&input_path, "English हिन्दी తెలుగు").unwrap();
        let state = crate::initialize_runtime_with_resources(
            &app_data,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Utc::now(),
        )
        .unwrap();
        let input = state.viewer_sessions.open_txt(&input_path).unwrap();
        let grant = state
            .viewer_sessions
            .grant_destination(&destination)
            .unwrap();

        for (index, checkpoint, expected_state) in [
            (0, ServiceCheckpoint::QpdfNormalization, JobState::Cancelled),
            (
                1,
                ServiceCheckpoint::OutputVerification,
                JobState::Cancelled,
            ),
            (2, ServiceCheckpoint::PrePublication, JobState::Cancelled),
            (
                3,
                ServiceCheckpoint::PostPublicationAudit,
                JobState::Completed,
            ),
            (4, ServiceCheckpoint::Cleanup, JobState::Completed),
        ] {
            let source = state
                .viewer_sessions
                .source_for_text_job(&input.session_id, input.generation)
                .unwrap();
            let service = TextToPdfService::new(state.clone());
            let created = service
                .create_job(crate::contracts::TextToPdfJobCreateRequest {
                    operation_id: TEXT_TO_PDF_OPERATION_ID.to_owned(),
                    input_session_id: input.session_id.clone(),
                    input_generation: input.generation,
                    destination_grant_id: grant.grant_id.clone(),
                    requested_output_name: format!("cancel-{index}.pdf"),
                    settings: TextToPdfSettings {
                        page_size: crate::text_to_pdf::TextPageSize::A4,
                        orientation: crate::text_to_pdf::TextOrientation::Portrait,
                    },
                })
                .unwrap();
            let token = state.cancellations.register(&created.id);
            *SERVICE_TEST_CANCELLATION
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = Some((created.id.clone(), checkpoint));
            let terminal = service
                .execute_with_registered_token(&created.id, source, token, |_| {})
                .unwrap_or_else(|error| panic!("{checkpoint:?}: {}", error.code));
            *SERVICE_TEST_CANCELLATION.get().unwrap().lock().unwrap() = None;
            assert_eq!(terminal.state, expected_state, "{checkpoint:?}");
            assert!(
                !state.workspaces.root().join(&created.id).exists(),
                "owned workspace remains at {checkpoint:?}"
            );
            match expected_state {
                JobState::Completed => {
                    let output = &terminal.outputs[0];
                    assert_eq!(output.status, OutputStatus::Published);
                    let final_path = Path::new(output.final_path.as_deref().unwrap());
                    assert!(final_path.exists());
                    assert_eq!(
                        hash_file(final_path).unwrap(),
                        (output.size_bytes.unwrap(), output.sha256.clone().unwrap())
                    );
                }
                JobState::Cancelled => {
                    assert!(terminal.outputs[0].final_path.is_none());
                    assert!(!destination.join(format!("cancel-{index}.pdf")).exists());
                }
                _ => unreachable!(),
            }
        }
        state.viewer_sessions.close_all();
    }
}
