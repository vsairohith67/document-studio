use document_studio_lib::contracts::{
    FilesInspectRequest, JobRecord, JobState, OperationManifest, ProgressEvent,
};
use document_studio_lib::ipc::files_inspect;
use document_studio_lib::job_engine::{apply_transition, can_transition, TransitionError};
use serde_json::Value;

const GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/foundation-contracts.json");
const PDF_MERGE_GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/pdf-merge-contracts.json");

#[test]
fn rust_deserializes_and_round_trips_shared_golden_payloads() {
    let fixture: Value = serde_json::from_str(GOLDEN).expect("golden JSON must parse");
    let job: JobRecord =
        serde_json::from_value(fixture["job"].clone()).expect("golden job must deserialize");
    let event: ProgressEvent = serde_json::from_value(fixture["progressEvent"].clone())
        .expect("golden progress event must deserialize");
    let operation: OperationManifest = serde_json::from_value(fixture["operationManifest"].clone())
        .expect("golden operation must deserialize");

    assert_eq!(job.state, JobState::Running);
    assert_eq!(job.operation_version, "1.0.1");
    assert_eq!(event.job_id, job.id);
    assert_eq!(operation.id, "diagnostic.copy");
    assert_eq!(operation.version, "1.0.1");
    assert_eq!(serde_json::to_value(&job).unwrap(), fixture["job"]);
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        fixture["progressEvent"]
    );
    assert_eq!(
        serde_json::to_value(&operation).unwrap(),
        fixture["operationManifest"]
    );
}

#[test]
fn rust_deserializes_the_pdf_merge_manifest_and_ordered_request() {
    let fixture: Value = serde_json::from_str(PDF_MERGE_GOLDEN).expect("golden JSON must parse");
    let operation: OperationManifest = serde_json::from_value(fixture["operationManifest"].clone())
        .expect("PDF Merge operation must deserialize");
    let request: document_studio_lib::contracts::JobsCreateRequest =
        serde_json::from_value(fixture["minimumRequest"].clone())
            .expect("PDF Merge request must deserialize");

    assert_eq!(operation.id, "pdf.merge");
    assert_eq!(operation.inputs.minimum, 2);
    assert_eq!(operation.inputs.maximum, 128);
    assert_eq!(request.operation_id, "pdf.merge");
    assert_eq!(request.input_paths.len(), 2);
    assert!(request.input_paths[0].ends_with("cover.pdf"));
    assert!(request.input_paths[1].ends_with("body.pdf"));
}

#[test]
fn compare_and_set_rejects_stale_or_illegal_transitions() {
    let fixture: Value = serde_json::from_str(GOLDEN).unwrap();
    let mut job: JobRecord = serde_json::from_value(fixture["job"].clone()).unwrap();

    assert!(can_transition(JobState::Running, JobState::Verifying));
    apply_transition(
        &mut job,
        JobState::Running,
        4,
        JobState::Verifying,
        "2026-08-16T12:00:05Z".to_owned(),
    )
    .unwrap();
    assert_eq!(job.version, 5);

    let stale = apply_transition(
        &mut job,
        JobState::Running,
        4,
        JobState::Verifying,
        "2026-08-16T12:00:06Z".to_owned(),
    );
    assert!(matches!(stale, Err(TransitionError::StateConflict { .. })));

    let illegal = apply_transition(
        &mut job,
        JobState::Verifying,
        5,
        JobState::Completed,
        "2026-08-16T12:00:07Z".to_owned(),
    );
    assert!(matches!(
        illegal,
        Err(TransitionError::IllegalTransition { .. })
    ));
}

#[test]
fn file_inspection_reports_pdf_only_for_extension_and_magic() {
    let directory = tempfile::tempdir().unwrap();
    let valid_pdf = directory.path().join("unicode-文件.PDF");
    let false_pdf = directory.path().join("not-really.pdf");
    let magic_without_extension = directory.path().join("hidden.bin");
    std::fs::write(&valid_pdf, b"prefix\n%PDF-1.7\n").unwrap();
    std::fs::write(&false_pdf, b"plain text").unwrap();
    std::fs::write(&magic_without_extension, b"%PDF-1.7\n").unwrap();

    let inspected = files_inspect(FilesInspectRequest {
        paths: vec![
            valid_pdf.to_string_lossy().into_owned(),
            false_pdf.to_string_lossy().into_owned(),
            magic_without_extension.to_string_lossy().into_owned(),
        ],
    })
    .unwrap();

    assert_eq!(inspected[0].mime_type, "application/pdf");
    assert_eq!(inspected[1].mime_type, "application/octet-stream");
    assert_eq!(inspected[2].mime_type, "application/octet-stream");
}

#[test]
fn file_inspection_rejects_more_than_128_paths_before_reading() {
    let error = files_inspect(FilesInspectRequest {
        paths: vec![r"C:\missing.pdf".to_owned(); 129],
    })
    .unwrap_err();
    assert_eq!(error.code, "INVALID_REQUEST");
}
