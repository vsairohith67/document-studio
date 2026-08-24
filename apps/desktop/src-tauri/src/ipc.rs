use std::fs;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use tauri::{
    http::HeaderMap,
    ipc::{InvokeBody, Request, Response},
    AppHandle, Emitter, State,
};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::{AppState, CancelOutcome};
use crate::contracts::{
    CancelResponse, CorePdfJobCreateRequest, DestinationGrant, DestinationGrantRequest,
    FileInspection, FilesInspectRequest, HistoryDeleteRequest, HistoryListRequest, JobIdRequest,
    JobRecord, JobWarning, JobsCreateRequest, OperationError, OperationManifest, OperationStage,
    PdfToImagesJobCreateRequest, PdfToImagesJobSession, SettingGetRequest, SettingRecord,
    SettingSetRequest, SystemStatus, ViewerDocumentMetadata, ViewerRangeRequest,
    ViewerSessionRequest, JOB_PROGRESS_EVENT_NAME, PDF_EXTRACT_OPERATION_ID,
    PDF_REMOVE_OPERATION_ID, PDF_REORDER_OPERATION_ID, PDF_ROTATE_OPERATION_ID,
    PDF_SPLIT_OPERATION_ID, PDF_TO_IMAGES_OPERATION_ID,
};
use crate::diagnostic_copy::DiagnosticCopyService;
use crate::diagnostics::scan_dependencies;
use crate::image_to_pdf::ImageToPdfService;
use crate::operation_registry::{all_manifests, validate_create_request, OperationKind};
use crate::path_policy::canonical_regular_file;
use crate::pdf_compression::PdfCompressionService;
use crate::pdf_merge::PdfMergeService;
use crate::pdf_operations::PdfPageOperationService;
use crate::pdf_to_images::{PdfToImagesService, PixelUploadMetadata};
use crate::recovery::{cancel_without_worker, resolve_interrupted, resolve_worker_spawn_failure};

#[tauri::command]
pub fn system_status(state: State<'_, AppState>) -> Result<SystemStatus, OperationError> {
    let version = state
        .database()
        .migration_versions()
        .map_err(|_| metadata_error())?
        .last()
        .copied()
        .unwrap_or(0);
    Ok(SystemStatus {
        product: "Document Studio".to_owned(),
        phase: "g04b2-pdf-to-images".to_owned(),
        offline_by_default: true,
        database_schema_version: u32::try_from(version).unwrap_or(0),
        webview2_runtime_version: tauri::webview_version().ok(),
    })
}

#[tauri::command]
pub fn operations_list() -> Vec<OperationManifest> {
    all_manifests()
}

#[tauri::command]
pub async fn viewer_open_dialog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ViewerDocumentMetadata>, OperationError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("PDF documents", &["pdf"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|_| path_error())?;
    state.viewer_sessions.open_pdf(&path).map(Some)
}

#[cfg(feature = "test-runtime")]
#[tauri::command]
pub fn viewer_open_test_fixture(
    state: State<'_, AppState>,
) -> Result<ViewerDocumentMetadata, OperationError> {
    let path = std::env::var_os("DOCUMENT_STUDIO_TEST_VIEWER_PATH")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| {
            OperationError::safe(
                "TEST_FIXTURE_UNAVAILABLE",
                "The test PDF fixture is unavailable",
                "Set the isolated test-runtime fixture path before launching the smoke test.",
                OperationStage::Inspect,
                false,
            )
        })?;
    state.viewer_sessions.open_pdf(&path)
}

#[cfg(feature = "test-runtime")]
#[tauri::command]
pub fn viewer_grant_test_destination(
    state: State<'_, AppState>,
) -> Result<DestinationGrant, OperationError> {
    let app_data = std::env::var_os("DOCUMENT_STUDIO_TEST_APP_DATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(test_destination_error)?;
    let destination = std::env::var_os("DOCUMENT_STUDIO_TEST_OUTPUT_DIRECTORY")
        .map(std::path::PathBuf::from)
        .ok_or_else(test_destination_error)?;
    let app_data =
        crate::path_policy::canonical_directory(&app_data).map_err(|_| test_destination_error())?;
    let destination = crate::path_policy::canonical_directory(&destination)
        .map_err(|_| test_destination_error())?;
    if destination == app_data || !destination.starts_with(&app_data) {
        return Err(test_destination_error());
    }
    state.viewer_sessions.grant_destination(&destination)
}

#[tauri::command]
pub fn viewer_read_range(
    request: ViewerRangeRequest,
    state: State<'_, AppState>,
) -> Result<Response, OperationError> {
    state
        .viewer_sessions
        .read_range(&request)
        .map(Response::new)
}

#[tauri::command]
pub fn viewer_close(
    request: ViewerSessionRequest,
    state: State<'_, AppState>,
) -> Result<(), OperationError> {
    state.viewer_sessions.close(&request)
}

#[tauri::command]
pub fn viewer_set_drop_enabled(enabled: bool, state: State<'_, AppState>) {
    state.viewer_sessions.set_drop_enabled(enabled);
}

#[tauri::command]
pub async fn viewer_choose_destination(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DestinationGrant>, OperationError> {
    let selected = app.dialog().file().blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|_| path_error())?;
    state.viewer_sessions.grant_destination(&path).map(Some)
}

#[tauri::command]
pub fn viewer_revoke_destination(request: DestinationGrantRequest, state: State<'_, AppState>) {
    state.viewer_sessions.revoke_destination(&request.grant_id);
}

#[tauri::command]
pub fn files_inspect(request: FilesInspectRequest) -> Result<Vec<FileInspection>, OperationError> {
    if request.paths.is_empty() || request.paths.len() > 128 {
        return Err(request_error());
    }
    request
        .paths
        .iter()
        .map(|path| {
            let (canonical, identity) =
                canonical_regular_file(Path::new(path)).map_err(|_| path_error())?;
            let metadata = fs::metadata(&canonical).map_err(|_| path_error())?;
            let modified: DateTime<Utc> = metadata.modified().map_err(|_| path_error())?.into();
            Ok(FileInspection {
                path: canonical.to_string_lossy().into_owned(),
                display_name: canonical
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(path_error)?
                    .to_owned(),
                size_bytes: metadata.len(),
                modified_at: modified.to_rfc3339_opts(SecondsFormat::Secs, true),
                mime_type: inspected_mime_type(&canonical)?,
                file_identity: identity.to_string(),
            })
        })
        .collect()
}

#[tauri::command]
pub fn jobs_create(
    request: JobsCreateRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<JobRecord, OperationError> {
    let kind = validate_create_request(&request)?;
    let job = match kind {
        OperationKind::DiagnosticCopy => {
            DiagnosticCopyService::new(state.inner().clone()).create_job(request)?
        }
        OperationKind::PdfMerge => {
            PdfMergeService::new(state.inner().clone()).create_job(request)?
        }
        OperationKind::PdfCompressLossless => {
            PdfCompressionService::new(state.inner().clone()).create_job(request)?
        }
        OperationKind::ImageToPdf => {
            ImageToPdfService::new(state.inner().clone()).create_job(request)?
        }
    };
    let job_id = job.id.clone();
    let worker_job_id = job_id.clone();
    let worker_state = state.inner().clone();
    spawn_registered_worker(
        state.inner(),
        &job_id,
        move |token| match kind {
            OperationKind::DiagnosticCopy => {
                let service = DiagnosticCopyService::new(worker_state);
                let _ = service.execute_with_registered_token(&worker_job_id, token, |event| {
                    let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
                });
            }
            OperationKind::PdfMerge => {
                let service = PdfMergeService::new(worker_state);
                let _ = service.execute_with_registered_token(&worker_job_id, token, |event| {
                    let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
                });
            }
            OperationKind::PdfCompressLossless => {
                let service = PdfCompressionService::new(worker_state);
                let _ = service.execute_with_registered_token(&worker_job_id, token, |event| {
                    let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
                });
            }
            OperationKind::ImageToPdf => {
                let service = ImageToPdfService::new(worker_state);
                let _ = service.execute_with_registered_token(&worker_job_id, token, |event| {
                    let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
                });
            }
        },
        |name, worker| {
            std::thread::Builder::new()
                .name(name)
                .spawn(worker)
                .map(|_| ())
        },
    )?;
    Ok(job)
}

#[tauri::command]
pub fn jobs_create_core_pdf(
    request: CorePdfJobCreateRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<JobRecord, OperationError> {
    let service = PdfPageOperationService::new(state.inner().clone());
    let (job, source) = service.create_job(request)?;
    let job_id = job.id.clone();
    let worker_job_id = job_id.clone();
    let worker_state = state.inner().clone();
    spawn_registered_worker(
        state.inner(),
        &job_id,
        move |token| {
            let service = PdfPageOperationService::new(worker_state);
            let _ = service.execute_with_registered_token(&worker_job_id, source, token, |event| {
                let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
            });
        },
        |name, worker| {
            std::thread::Builder::new()
                .name(name)
                .spawn(worker)
                .map(|_| ())
        },
    )?;
    Ok(redact_viewer_job_paths(job))
}

#[tauri::command]
pub fn jobs_create_pdf_to_images(
    request: PdfToImagesJobCreateRequest,
    state: State<'_, AppState>,
) -> Result<PdfToImagesJobSession, OperationError> {
    let mut session = PdfToImagesService::new(state.inner().clone()).create_job(request)?;
    session.job = redact_viewer_job_paths(session.job);
    Ok(session)
}

#[tauri::command]
pub async fn pdf_to_images_submit_page(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<JobRecord, OperationError> {
    let metadata = pixel_upload_metadata(&request)?;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) if bytes.len() <= 67_108_864 => bytes.clone(),
        _ => return Err(pixel_body_error()),
    };
    let worker_state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        PdfToImagesService::new(worker_state).submit_page(metadata, bytes, |event| {
            let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
        })
    })
    .await
    .map_err(|_| pixel_body_error())??;
    Ok(redact_viewer_job_paths(result))
}

fn pixel_upload_metadata(request: &Request<'_>) -> Result<PixelUploadMetadata, OperationError> {
    pixel_upload_metadata_from_headers(request.headers())
}

fn pixel_upload_metadata_from_headers(
    headers: &HeaderMap,
) -> Result<PixelUploadMetadata, OperationError> {
    fn header(headers: &HeaderMap, name: &'static str) -> Result<String, OperationError> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .map(str::to_owned)
            .ok_or_else(pixel_header_error)
    }
    fn u32_header(headers: &HeaderMap, name: &'static str) -> Result<u32, OperationError> {
        header(headers, name)?
            .parse::<u32>()
            .map_err(|_| pixel_header_error())
    }
    Ok(PixelUploadMetadata {
        job_id: header(headers, "x-document-studio-job-id")?,
        render_session_id: header(headers, "x-document-studio-render-session-id")?,
        page_ordinal: u32_header(headers, "x-document-studio-page-ordinal")?,
        nonce: header(headers, "x-document-studio-page-nonce")?,
        expected_width: u32_header(headers, "x-document-studio-expected-width")?,
        expected_height: u32_header(headers, "x-document-studio-expected-height")?,
    })
}

fn spawn_registered_worker<W, S>(
    state: &AppState,
    job_id: &str,
    worker: W,
    spawner: S,
) -> Result<(), OperationError>
where
    W: FnOnce(crate::app_state::CancellationToken) + Send + 'static,
    S: FnOnce(String, Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
{
    let token = state.cancellations.register(job_id);
    let task = Box::new(move || worker(token));
    if spawner(format!("document-studio-job-{job_id}"), task).is_err() {
        state.cancellations.unregister(job_id);
        resolve_worker_spawn_failure(state, job_id)?;
        return Err(OperationError::safe(
            "JOB_START_FAILED",
            "The job could not be started",
            "No processing began. Temporary data was reconciled safely.",
            OperationStage::Plan,
            true,
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn jobs_cancel(
    request: JobIdRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CancelResponse, OperationError> {
    match state.cancellations.request(&request.job_id) {
        CancelOutcome::Requested => {
            state
                .database()
                .request_cancellation(&request.job_id)
                .map_err(|_| metadata_error())?;
            let is_pdf_to_images = state
                .database()
                .get_job(&request.job_id)
                .map_err(|_| metadata_error())?
                .is_some_and(|job| job.operation_id == PDF_TO_IMAGES_OPERATION_ID);
            if is_pdf_to_images {
                let _ = PdfToImagesService::new(state.inner().clone()).cancel_if_idle(
                    &request.job_id,
                    |event| {
                        let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
                    },
                )?;
            }
            Ok(CancelResponse {
                outcome: "requested".to_owned(),
            })
        }
        CancelOutcome::TooLate => Err(OperationError::safe(
            "CANCEL_TOO_LATE",
            "The output is already being committed",
            "The final no-overwrite publication step cannot be cancelled safely.",
            OperationStage::Publish,
            false,
        )),
        CancelOutcome::NotRunning => cancel_queued_job(&request.job_id, state.inner()),
    }
}

#[tauri::command]
pub fn jobs_resolve_interrupted(
    request: JobIdRequest,
    state: State<'_, AppState>,
) -> Result<JobRecord, OperationError> {
    resolve_interrupted(state.inner(), &request.job_id).map(redact_viewer_job_paths)
}

#[tauri::command]
pub fn jobs_get(
    request: JobIdRequest,
    state: State<'_, AppState>,
) -> Result<JobRecord, OperationError> {
    state
        .database()
        .get_job(&request.job_id)
        .map_err(|_| metadata_error())?
        .ok_or_else(job_not_found)
        .map(redact_viewer_job_paths)
}

#[tauri::command]
pub fn jobs_warnings(
    request: JobIdRequest,
    state: State<'_, AppState>,
) -> Result<Vec<JobWarning>, OperationError> {
    state
        .database()
        .get_job(&request.job_id)
        .map_err(|_| metadata_error())?
        .ok_or_else(job_not_found)?;
    state
        .database()
        .list_warnings(&request.job_id)
        .map_err(|_| metadata_error())
}

#[tauri::command]
pub fn history_list(
    request: HistoryListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<JobRecord>, OperationError> {
    if request.limit == 0 || request.limit > 200 {
        return Err(request_error());
    }
    let _cursor = request.before_updated_at;
    state
        .database()
        .list_jobs(request.limit)
        .map(|jobs| jobs.into_iter().map(redact_viewer_job_paths).collect())
        .map_err(|_| metadata_error())
}

fn redact_viewer_job_paths(mut job: JobRecord) -> JobRecord {
    if !matches!(
        job.operation_id.as_str(),
        PDF_EXTRACT_OPERATION_ID
            | PDF_REMOVE_OPERATION_ID
            | PDF_REORDER_OPERATION_ID
            | PDF_ROTATE_OPERATION_ID
            | PDF_SPLIT_OPERATION_ID
            | PDF_TO_IMAGES_OPERATION_ID
    ) {
        return job;
    }
    job.destination_directory.clear();
    for input in &mut job.inputs {
        input.source_path.clear();
        input.canonical_path.clear();
    }
    for output in &mut job.outputs {
        output.staging_path = None;
        output.partial_path = None;
        output.final_path = None;
    }
    job
}

#[tauri::command]
pub fn history_delete(
    request: HistoryDeleteRequest,
    state: State<'_, AppState>,
) -> Result<usize, OperationError> {
    if request.job_ids.is_empty() || request.job_ids.len() > 200 {
        return Err(request_error());
    }
    match state.database().delete_terminal_history(&request.job_ids) {
        Ok(deleted) => Ok(deleted),
        Err(crate::database::DatabaseError::LegacyCleanupUnproven) => {
            Err(legacy_cleanup_unproven_error())
        }
        Err(_) => Err(metadata_error()),
    }
}

#[tauri::command]
pub fn dependencies_scan(
    state: State<'_, AppState>,
) -> Result<Vec<crate::contracts::DependencyDiagnostic>, OperationError> {
    scan_dependencies(state.inner())
}

#[tauri::command]
pub fn settings_get(
    request: SettingGetRequest,
    state: State<'_, AppState>,
) -> Result<Option<SettingRecord>, OperationError> {
    state
        .database()
        .get_setting(&request.scope, &request.key)
        .map_err(|_| request_error())
}

#[tauri::command]
pub fn settings_set(
    request: SettingSetRequest,
    state: State<'_, AppState>,
) -> Result<SettingRecord, OperationError> {
    let is_retention = request.scope == crate::contracts::HISTORY_RETENTION_SCOPE
        && request.key == crate::contracts::HISTORY_RETENTION_KEY;
    let mut database = state.database();
    let setting = database
        .set_setting(
            &request.scope,
            &request.key,
            request.value,
            request.expected_version,
        )
        .map_err(|_| request_error())?;
    if is_retention {
        database
            .run_retention_at(Utc::now())
            .map_err(|_| metadata_error())?;
    }
    Ok(setting)
}

pub fn diagnostic_copy_manifest() -> OperationManifest {
    crate::operation_registry::diagnostic_copy_manifest()
}

fn inspected_mime_type(path: &Path) -> Result<String, OperationError> {
    match crate::pdf_merge::inspect_pdf_mime(path) {
        Ok(value) => Ok(value.to_owned()),
        Err(_) => crate::image_to_pdf::inspect_image_mime(path).map(str::to_owned),
    }
}

fn cancel_queued_job(job_id: &str, state: &AppState) -> Result<CancelResponse, OperationError> {
    cancel_without_worker(state, job_id)?;
    Ok(CancelResponse {
        outcome: "cancelled".to_owned(),
    })
}

fn legacy_cleanup_unproven_error() -> OperationError {
    OperationError::safe(
        crate::contracts::LEGACY_CLEANUP_UNPROVEN,
        "Legacy destination cleanup is unproven",
        "Inspect the affected destination manually; the history record was preserved.",
        OperationStage::Recovery,
        false,
    )
}

fn request_error() -> OperationError {
    OperationError::safe(
        "INVALID_REQUEST",
        "The request is not valid",
        "Review the selected operation values and try again.",
        OperationStage::Preflight,
        false,
    )
}

fn path_error() -> OperationError {
    OperationError::safe(
        "PATH_UNSAFE",
        "The selected path is not safe",
        "Choose a regular local file without links or special path syntax.",
        OperationStage::Inspect,
        false,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "Job metadata could not be read or saved",
        "The operation cannot continue until its local metadata is available.",
        OperationStage::Audit,
        true,
    )
}

fn job_not_found() -> OperationError {
    OperationError::safe(
        "JOB_NOT_FOUND",
        "The job is not available",
        "Refresh job history and try again.",
        OperationStage::Recovery,
        false,
    )
}

fn pixel_body_error() -> OperationError {
    OperationError::safe(
        "PIXEL_IPC_INVALID",
        "The raw page pixel transfer is invalid",
        "Only an authenticated bounded raw RGBA body is accepted.",
        OperationStage::Execute,
        false,
    )
}

fn pixel_header_error() -> OperationError {
    OperationError::safe(
        "PIXEL_IPC_AUTH_INVALID",
        "The raw page pixel identity is invalid",
        "All bounded job, session, page, nonce, width, and height headers are required.",
        OperationStage::Execute,
        false,
    )
}

#[cfg(feature = "test-runtime")]
fn test_destination_error() -> OperationError {
    OperationError::safe(
        "TEST_DESTINATION_UNAVAILABLE",
        "The isolated test destination is unavailable",
        "Use a newly created destination inside the isolated test app-data boundary.",
        OperationStage::Preflight,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        diagnostic_copy_manifest, pixel_upload_metadata_from_headers, redact_viewer_job_paths,
        spawn_registered_worker,
    };
    use crate::app_state::{AppState, CancelOutcome};
    use crate::contracts::{
        JobInput, JobOutput, JobProgress, JobRecord, JobState, JobsCreateRequest, OperationStage,
        OutputStatus, ProgressUnit, PDF_EXTRACT_OPERATION_ID, PDF_MERGE_OPERATION_ID,
        PDF_TO_IMAGES_OPERATION_ID,
    };
    use crate::database::Database;
    use crate::diagnostic_copy::DiagnosticCopyService;
    use crate::workspace::WorkspaceManager;
    use tauri::http::{HeaderMap, HeaderValue};
    use tempfile::tempdir;

    #[test]
    fn diagnostic_manifest_has_the_complete_reference_lifecycle() {
        let manifest = diagnostic_copy_manifest();
        assert_eq!(manifest.id, "diagnostic.copy");
        assert_eq!(manifest.version, "1.0.1");
        assert_eq!(manifest.inputs.minimum, 1);
        assert_eq!(manifest.inputs.maximum, 1);
        assert_eq!(manifest.stages.first(), Some(&OperationStage::Inspect));
        assert_eq!(manifest.stages.last(), Some(&OperationStage::Cleanup));
        assert!(manifest.dependencies.iter().all(|id| !id.contains("qpdf")));
    }

    fn path_bearing_job(operation_id: &str) -> JobRecord {
        JobRecord {
            id: "job-id".to_owned(),
            operation_id: operation_id.to_owned(),
            operation_version: "1.0.0".to_owned(),
            state: JobState::Completed,
            stage: Some(OperationStage::Cleanup),
            sequence: 1,
            progress: JobProgress {
                completed_units: 1,
                total_units: 1,
                unit: ProgressUnit::Steps,
            },
            destination_directory: r"C:\Secret\Output".to_owned(),
            requested_output_name: "result.pdf".to_owned(),
            resolved_output_name: Some("result.pdf".to_owned()),
            cancellation_requested_at: None,
            created_at: "2026-08-17T00:00:00Z".to_owned(),
            updated_at: "2026-08-17T00:00:01Z".to_owned(),
            finished_at: Some("2026-08-17T00:00:01Z".to_owned()),
            version: 1,
            inputs: vec![JobInput {
                ordinal: 0,
                display_name: "private.pdf".to_owned(),
                source_path: r"C:\Secret\private.pdf".to_owned(),
                canonical_path: r"C:\Secret\private.pdf".to_owned(),
                file_identity: "opaque-identity".to_owned(),
                size_bytes: 42,
                modified_at: "2026-08-17T00:00:00Z".to_owned(),
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                password_reference: None,
            }],
            outputs: vec![JobOutput {
                ordinal: 0,
                requested_name: "result.pdf".to_owned(),
                resolved_name: Some("result.pdf".to_owned()),
                staging_path: Some(r"C:\PrivateWorkspace\result.pdf".to_owned()),
                partial_path: Some(r"C:\Secret\Output\result.partial".to_owned()),
                final_path: Some(r"C:\Secret\Output\result.pdf".to_owned()),
                size_bytes: Some(42),
                mime_type: "application/pdf".to_owned(),
                sha256: Some("0".repeat(64)),
                status: OutputStatus::Published,
                verified_at: Some("2026-08-17T00:00:01Z".to_owned()),
                published_at: Some("2026-08-17T00:00:01Z".to_owned()),
            }],
            errors: Vec::new(),
        }
    }

    #[test]
    fn opaque_viewer_job_responses_redact_paths_without_changing_accepted_merge_records() {
        let redacted = redact_viewer_job_paths(path_bearing_job(PDF_EXTRACT_OPERATION_ID));
        assert!(redacted.destination_directory.is_empty());
        assert!(redacted.inputs[0].source_path.is_empty());
        assert!(redacted.inputs[0].canonical_path.is_empty());
        assert!(redacted.outputs[0].staging_path.is_none());
        assert!(redacted.outputs[0].partial_path.is_none());
        assert!(redacted.outputs[0].final_path.is_none());
        assert!(!serde_json::to_string(&redacted)
            .unwrap()
            .contains(r"C:\Secret"));

        let pdf_images = redact_viewer_job_paths(path_bearing_job(PDF_TO_IMAGES_OPERATION_ID));
        assert!(pdf_images.destination_directory.is_empty());
        assert!(pdf_images.inputs[0].source_path.is_empty());
        assert!(pdf_images.inputs[0].canonical_path.is_empty());
        assert!(pdf_images.outputs[0].staging_path.is_none());
        assert!(pdf_images.outputs[0].partial_path.is_none());
        assert!(pdf_images.outputs[0].final_path.is_none());
        assert!(!serde_json::to_string(&pdf_images)
            .unwrap()
            .contains(r"C:\Secret"));

        let accepted_merge = redact_viewer_job_paths(path_bearing_job(PDF_MERGE_OPERATION_ID));
        assert_eq!(accepted_merge.destination_directory, r"C:\Secret\Output");
        assert_eq!(
            accepted_merge.inputs[0].source_path,
            r"C:\Secret\private.pdf"
        );
    }

    fn pixel_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-document-studio-job-id",
            HeaderValue::from_static("job-id"),
        );
        headers.insert(
            "x-document-studio-render-session-id",
            HeaderValue::from_static("render-session"),
        );
        headers.insert(
            "x-document-studio-page-ordinal",
            HeaderValue::from_static("0"),
        );
        headers.insert(
            "x-document-studio-page-nonce",
            HeaderValue::from_static("one-use-nonce"),
        );
        headers.insert(
            "x-document-studio-expected-width",
            HeaderValue::from_static("8192"),
        );
        headers.insert(
            "x-document-studio-expected-height",
            HeaderValue::from_static("2048"),
        );
        headers
    }

    #[test]
    fn raw_pixel_headers_are_complete_bounded_and_strictly_numeric() {
        let valid = pixel_upload_metadata_from_headers(&pixel_headers()).unwrap();
        assert_eq!(valid.job_id, "job-id");
        assert_eq!(valid.render_session_id, "render-session");
        assert_eq!(valid.page_ordinal, 0);
        assert_eq!(valid.nonce, "one-use-nonce");
        assert_eq!((valid.expected_width, valid.expected_height), (8192, 2048));

        for missing in [
            "x-document-studio-job-id",
            "x-document-studio-render-session-id",
            "x-document-studio-page-ordinal",
            "x-document-studio-page-nonce",
            "x-document-studio-expected-width",
            "x-document-studio-expected-height",
        ] {
            let mut headers = pixel_headers();
            headers.remove(missing);
            assert_eq!(
                pixel_upload_metadata_from_headers(&headers)
                    .unwrap_err()
                    .code,
                "PIXEL_IPC_AUTH_INVALID"
            );
        }
        let mut invalid_number = pixel_headers();
        invalid_number.insert(
            "x-document-studio-expected-width",
            HeaderValue::from_static("8192px"),
        );
        assert_eq!(
            pixel_upload_metadata_from_headers(&invalid_number)
                .unwrap_err()
                .code,
            "PIXEL_IPC_AUTH_INVALID"
        );
        let mut oversized = pixel_headers();
        oversized.insert(
            "x-document-studio-page-nonce",
            HeaderValue::from_str(&"n".repeat(129)).unwrap(),
        );
        assert_eq!(
            pixel_upload_metadata_from_headers(&oversized)
                .unwrap_err()
                .code,
            "PIXEL_IPC_AUTH_INVALID"
        );
    }

    #[test]
    fn worker_token_exists_before_spawn_and_spawn_failure_resolves_queued_job() {
        let app_data = tempdir().unwrap();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let input = source.path().join("input.bin");
        std::fs::write(&input, b"spawn failure").unwrap();
        let state = AppState::new(
            Database::open(&app_data.path().join("metadata.sqlite3")).unwrap(),
            WorkspaceManager::initialize(app_data.path()).unwrap(),
        );
        let job = DiagnosticCopyService::new(state.clone())
            .create_job(JobsCreateRequest {
                operation_id: "diagnostic.copy".to_owned(),
                input_paths: vec![input.to_string_lossy().into_owned()],
                destination_directory: destination.path().to_string_lossy().into_owned(),
                requested_output_name: "copy.bin".to_owned(),
            })
            .unwrap();
        let observed_state = state.clone();
        let observed_job_id = job.id.clone();

        let error = spawn_registered_worker(
            &state,
            &job.id,
            |_token| {},
            move |_name, _worker| {
                assert_eq!(
                    observed_state.cancellations.request(&observed_job_id),
                    CancelOutcome::Requested
                );
                Err(std::io::Error::other("injected spawn failure"))
            },
        )
        .unwrap_err();

        assert_eq!(error.code, "JOB_START_FAILED");
        assert_eq!(
            state.database().get_job(&job.id).unwrap().unwrap().state,
            JobState::Failed
        );
        assert_eq!(
            state.cancellations.request(&job.id),
            CancelOutcome::NotRunning
        );
    }
}
