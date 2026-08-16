use std::fs;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::app_state::{AppState, CancelOutcome};
use crate::contracts::{
    CancelResponse, FileInspection, FilesInspectRequest, HistoryDeleteRequest, HistoryListRequest,
    JobIdRequest, JobRecord, JobsCreateRequest, OperationError, OperationInputs, OperationManifest,
    OperationOutputs, OperationStage, SettingGetRequest, SettingRecord, SettingSetRequest,
    SystemStatus, DIAGNOSTIC_COPY_OPERATION_ID, DIAGNOSTIC_COPY_VERSION, JOB_PROGRESS_EVENT_NAME,
};
use crate::diagnostic_copy::DiagnosticCopyService;
use crate::diagnostics::scan_dependencies;
use crate::path_policy::canonical_regular_file;
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
        phase: "foundation".to_owned(),
        offline_by_default: true,
        database_schema_version: u32::try_from(version).unwrap_or(0),
    })
}

#[tauri::command]
pub fn operations_list() -> Vec<OperationManifest> {
    vec![diagnostic_copy_manifest()]
}

#[tauri::command]
pub fn files_inspect(request: FilesInspectRequest) -> Result<Vec<FileInspection>, OperationError> {
    if request.paths.is_empty() || request.paths.len() > 32 {
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
                mime_type: "application/octet-stream".to_owned(),
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
    let service = DiagnosticCopyService::new(state.inner().clone());
    let job = service.create_job(request)?;
    let job_id = job.id.clone();
    let worker_job_id = job_id.clone();
    spawn_registered_worker(
        state.inner(),
        &job_id,
        move |token| {
            let _ = service.execute_with_registered_token(&worker_job_id, token, |event| {
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
    Ok(job)
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
    state: State<'_, AppState>,
) -> Result<CancelResponse, OperationError> {
    match state.cancellations.request(&request.job_id) {
        CancelOutcome::Requested => {
            state
                .database()
                .request_cancellation(&request.job_id)
                .map_err(|_| metadata_error())?;
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
    resolve_interrupted(state.inner(), &request.job_id)
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
        .map_err(|_| metadata_error())
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
    scan_dependencies(&mut state.database())
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
    OperationManifest {
        id: DIAGNOSTIC_COPY_OPERATION_ID.to_owned(),
        version: DIAGNOSTIC_COPY_VERSION.to_owned(),
        name: "Diagnostic copy".to_owned(),
        category: "diagnostics".to_owned(),
        description: "Streams, verifies, and safely publishes one local file.".to_owned(),
        risk: "normal".to_owned(),
        locality: "local".to_owned(),
        inputs: OperationInputs {
            accepted_mime_types: vec!["application/octet-stream".to_owned()],
            minimum: 1,
            maximum: 1,
            allow_directories: false,
        },
        settings_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
        outputs: OperationOutputs {
            mime_type: "application/octet-stream".to_owned(),
            multiplicity: "single".to_owned(),
        },
        dependencies: vec!["document-studio-core".to_owned()],
        verification: vec!["sha256".to_owned(), "size".to_owned(), "reopen".to_owned()],
        stages: vec![
            OperationStage::Inspect,
            OperationStage::Preflight,
            OperationStage::Estimate,
            OperationStage::Plan,
            OperationStage::Execute,
            OperationStage::Verify,
            OperationStage::Publish,
            OperationStage::Audit,
            OperationStage::Cleanup,
        ],
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

#[cfg(test)]
mod tests {
    use super::{diagnostic_copy_manifest, spawn_registered_worker};
    use crate::app_state::{AppState, CancelOutcome};
    use crate::contracts::{JobState, JobsCreateRequest, OperationStage};
    use crate::database::Database;
    use crate::diagnostic_copy::DiagnosticCopyService;
    use crate::workspace::WorkspaceManager;
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
