use std::fs;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::app_state::{AppState, CancelOutcome};
use crate::contracts::{
    CancelResponse, FileInspection, FilesInspectRequest, HistoryDeleteRequest, HistoryListRequest,
    JobIdRequest, JobRecord, JobState, JobsCreateRequest, OperationError, OperationInputs,
    OperationManifest, OperationOutputs, OperationStage, SettingGetRequest, SettingRecord,
    SettingSetRequest, SystemStatus, JOB_PROGRESS_EVENT_NAME,
};
use crate::diagnostic_copy::DiagnosticCopyService;
use crate::diagnostics::scan_dependencies;
use crate::job_engine::is_cancellable;
use crate::path_policy::canonical_regular_file;

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
    std::thread::Builder::new()
        .name(format!("document-studio-job-{job_id}"))
        .spawn(move || {
            let _ = service.execute(&job_id, |event| {
                let _ = app.emit(JOB_PROGRESS_EVENT_NAME, event);
            });
        })
        .map_err(|_| {
            OperationError::safe(
                "JOB_START_FAILED",
                "The job could not be started",
                "No processing began. Try the operation again.",
                OperationStage::Plan,
                true,
            )
        })?;
    Ok(job)
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
        CancelOutcome::NotRunning => cancel_queued_job(&request.job_id, &state),
    }
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
    state
        .database()
        .delete_terminal_history(&request.job_ids)
        .map_err(|_| metadata_error())
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
    state
        .database()
        .set_setting(
            &request.scope,
            &request.key,
            request.value,
            request.expected_version,
        )
        .map_err(|_| request_error())
}

pub fn diagnostic_copy_manifest() -> OperationManifest {
    OperationManifest {
        id: "diagnostic.copy".to_owned(),
        version: "1.0.0".to_owned(),
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

fn cancel_queued_job(
    job_id: &str,
    state: &State<'_, AppState>,
) -> Result<CancelResponse, OperationError> {
    let mut database = state.database();
    let job = database
        .get_job(job_id)
        .map_err(|_| metadata_error())?
        .ok_or_else(job_not_found)?;
    if !is_cancellable(job.state) {
        return Err(job_not_found());
    }
    database
        .clear_ephemeral_paths(job_id)
        .map_err(|_| metadata_error())?;
    let refreshed = database
        .get_job(job_id)
        .map_err(|_| metadata_error())?
        .ok_or_else(job_not_found)?;
    database
        .transition_job(
            job_id,
            refreshed.state,
            refreshed.version,
            JobState::Cancelled,
            Some(OperationStage::Cleanup),
        )
        .map_err(|_| metadata_error())?;
    Ok(CancelResponse {
        outcome: "cancelled".to_owned(),
    })
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
    use super::diagnostic_copy_manifest;
    use crate::contracts::OperationStage;

    #[test]
    fn diagnostic_manifest_has_the_complete_reference_lifecycle() {
        let manifest = diagnostic_copy_manifest();
        assert_eq!(manifest.id, "diagnostic.copy");
        assert_eq!(manifest.inputs.minimum, 1);
        assert_eq!(manifest.inputs.maximum, 1);
        assert_eq!(manifest.stages.first(), Some(&OperationStage::Inspect));
        assert_eq!(manifest.stages.last(), Some(&OperationStage::Cleanup));
        assert!(manifest.dependencies.iter().all(|id| !id.contains("qpdf")));
    }
}
