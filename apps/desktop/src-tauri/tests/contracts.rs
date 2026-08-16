use document_studio_lib::contracts::{JobRecord, JobState, OperationManifest, ProgressEvent};
use document_studio_lib::job_engine::{apply_transition, can_transition, TransitionError};
use serde_json::Value;

const GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/foundation-contracts.json");

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
