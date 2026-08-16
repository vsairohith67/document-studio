use chrono::{TimeZone, Utc};
use document_studio_lib::contracts::{JobRecord, JobState, OperationStage};
use document_studio_lib::database::Database;
use document_studio_lib::initialize_runtime;
use serde_json::Value;
use tempfile::tempdir;

const GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/foundation-contracts.json");

fn sample_job() -> JobRecord {
    let fixture: Value = serde_json::from_str(GOLDEN).unwrap();
    serde_json::from_value(fixture["job"].clone()).unwrap()
}

#[test]
fn actual_runtime_startup_recovers_before_bounded_retention_and_ipc_state() {
    let app_data = tempdir().unwrap();
    {
        let mut database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();

        let mut queued = sample_job();
        queued.id = "018f0f17-2f4a-7fb1-a247-606060606060".to_owned();
        queued.state = JobState::Queued;
        queued.stage = None;
        queued.sequence = 0;
        queued.version = 0;
        queued.progress.completed_units = 0;
        database.create_job(&queued).unwrap();

        let mut old = sample_job();
        old.id = "018f0f17-2f4a-7fb1-a247-707070707070".to_owned();
        old.state = JobState::Failed;
        old.stage = Some(OperationStage::Cleanup);
        old.updated_at = "2026-06-01T00:00:00Z".to_owned();
        old.finished_at = Some(old.updated_at.clone());
        database.create_job(&old).unwrap();

        let mut recent = sample_job();
        recent.id = "018f0f17-2f4a-7fb1-a247-808080808080".to_owned();
        recent.state = JobState::Failed;
        recent.stage = Some(OperationStage::Cleanup);
        recent.updated_at = "2026-08-10T00:00:00Z".to_owned();
        recent.finished_at = Some(recent.updated_at.clone());
        database.create_job(&recent).unwrap();
    }

    let maintenance_time = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let state = initialize_runtime(app_data.path(), maintenance_time).unwrap();
    let database = state.database();

    let queued = database
        .get_job("018f0f17-2f4a-7fb1-a247-606060606060")
        .unwrap()
        .unwrap();
    assert_eq!(queued.state, JobState::Failed);
    assert!(queued
        .errors
        .iter()
        .any(|error| error.code == "JOB_WORKER_NOT_STARTED"));
    assert!(database
        .get_job("018f0f17-2f4a-7fb1-a247-707070707070")
        .unwrap()
        .is_none());
    assert!(database
        .get_job("018f0f17-2f4a-7fb1-a247-808080808080")
        .unwrap()
        .is_some());
    assert_eq!(
        database
            .get_setting("application", "history.retention_days")
            .unwrap()
            .unwrap()
            .value,
        serde_json::json!(30)
    );
}
