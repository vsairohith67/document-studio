use std::fs;

use chrono::{TimeZone, Utc};
use document_studio_lib::contracts::{JobRecord, JobState, OperationStage};
use document_studio_lib::database::{
    apply_migrations, configure_connection, Database, DatabaseError, Migration,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use tempfile::tempdir;

const GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/foundation-contracts.json");

fn sample_job() -> JobRecord {
    let fixture: Value = serde_json::from_str(GOLDEN).unwrap();
    serde_json::from_value(fixture["job"].clone()).unwrap()
}

#[test]
fn fresh_database_migrates_to_version_three_and_reopens_idempotently() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("metadata.sqlite3");
    {
        let database = Database::open(&path).unwrap();
        assert_eq!(database.migration_versions().unwrap(), vec![1, 2, 3]);
        let integrity: String = database
            .connection()
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.migration_versions().unwrap(), vec![1, 2, 3]);
}

#[test]
fn migration_failure_rolls_back_the_entire_version() {
    let mut connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    let invalid = [Migration {
        version: 1,
        name: "invalid",
        sql: "CREATE TABLE should_rollback(id INTEGER); THIS IS NOT SQL;",
    }];

    assert!(apply_migrations(&mut connection, &invalid).is_err());
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'should_rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let version_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(table_count, 0);
    assert_eq!(version_count, 0);
}

#[test]
fn checksum_mismatch_fails_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("metadata.sqlite3");
    let database = Database::open(&path).unwrap();
    database
        .connection()
        .execute(
            "UPDATE schema_migrations SET sql_checksum = ?1 WHERE version = 2",
            ["0".repeat(64)],
        )
        .unwrap();
    drop(database);

    assert!(matches!(
        Database::open(&path),
        Err(DatabaseError::MigrationChecksum { version: 2 })
    ));
}

#[test]
fn schema_is_metadata_only_and_constraints_reject_invalid_state() {
    let database = Database::open_in_memory().unwrap();
    let mut statement = database
        .connection()
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'table' AND sql IS NOT NULL")
        .unwrap();
    let schemas = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(schemas
        .iter()
        .all(|(_name, sql)| !sql.to_ascii_uppercase().contains(" BLOB")));

    let invalid = database.connection().execute(
        "INSERT INTO jobs (
            id, operation_id, operation_version, state, sequence, completed_units,
            total_units, unit, destination_directory, requested_output_name,
            created_at, updated_at, version
         ) VALUES (?1, 'diagnostic.copy', '1.0.0', 'done', 0, 0, 0, 'steps', '.', 'copy.bin', ?2, ?2, 0)",
        ("018f0f17-2f4a-7fb1-a247-101010101010", "2026-08-16T12:00:00Z"),
    );
    assert!(invalid.is_err());
}

#[test]
fn job_repository_round_trips_and_compare_and_set_rejects_stale_updates() {
    let mut database = Database::open_in_memory().unwrap();
    let job = sample_job();
    database.create_job(&job).unwrap();
    assert_eq!(database.get_job(&job.id).unwrap().unwrap(), job);

    let next_version = database
        .transition_job(
            &job.id,
            JobState::Running,
            4,
            JobState::Verifying,
            Some(OperationStage::Verify),
        )
        .unwrap();
    assert_eq!(next_version, 5);
    let updated = database.get_job(&job.id).unwrap().unwrap();
    assert_eq!(updated.state, JobState::Verifying);
    assert_eq!(updated.version, 5);

    assert!(matches!(
        database.transition_job(
            &job.id,
            JobState::Running,
            4,
            JobState::Verifying,
            Some(OperationStage::Verify)
        ),
        Err(DatabaseError::JobConflict)
    ));
}

#[test]
fn retention_deletes_only_old_terminal_metadata() {
    let mut database = Database::open_in_memory().unwrap();
    let mut old_terminal = sample_job();
    old_terminal.id = "018f0f17-2f4a-7fb1-a247-202020202020".to_owned();
    old_terminal.state = JobState::Failed;
    old_terminal.stage = Some(OperationStage::Cleanup);
    old_terminal.finished_at = Some("2026-06-01T00:00:00Z".to_owned());
    old_terminal.updated_at = "2026-06-01T00:00:00Z".to_owned();
    database.create_job(&old_terminal).unwrap();

    let active = sample_job();
    database.create_job(&active).unwrap();
    assert_eq!(
        database
            .purge_terminal_before("2026-07-17T00:00:00Z")
            .unwrap(),
        1
    );
    assert!(database.get_job(&old_terminal.id).unwrap().is_none());
    assert!(database.get_job(&active.id).unwrap().is_some());
}

#[test]
fn setting_allow_list_prevents_document_or_secret_content_storage() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("metadata.sqlite3");
    let mut database = Database::open(&path).unwrap();
    let setting = database
        .set_setting("application", "history.retention_days", json!(30), 0)
        .unwrap();
    assert_eq!(setting.version, 1);
    assert_eq!(
        database
            .get_setting("application", "history.retention_days")
            .unwrap()
            .unwrap()
            .value,
        json!(30)
    );

    let sentinel = "DOCUMENT_BODY_SENTINEL_7b9f";
    assert!(matches!(
        database.set_setting("application", "document.body", json!(sentinel), 0),
        Err(DatabaseError::SettingNotAllowed)
    ));
    assert!(matches!(
        database.set_setting(
            "application",
            "history.retention_days",
            json!({ "password": sentinel }),
            1
        ),
        Err(DatabaseError::SettingNotAllowed)
    ));

    database
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(database);
    let bytes = fs::read(path).unwrap();
    assert!(!bytes
        .windows(sentinel.len())
        .any(|window| window == sentinel.as_bytes()));
}

#[test]
fn application_retention_defaults_to_thirty_and_rejects_wrong_scope() {
    let mut database = Database::open_in_memory().unwrap();
    let default = database.ensure_application_retention_setting().unwrap();
    assert_eq!(default.scope, "application");
    assert_eq!(default.key, "history.retention_days");
    assert_eq!(default.value, json!(30));

    assert!(matches!(
        database.get_setting("operation", "history.retention_days"),
        Err(DatabaseError::SettingNotAllowed)
    ));
    assert!(matches!(
        database.set_setting("operation", "history.retention_days", json!(30), 0),
        Err(DatabaseError::SettingNotAllowed)
    ));
}

#[test]
fn application_retention_accepts_zero_and_custom_values_but_rejects_invalid_values() {
    for value in [
        json!(-1),
        json!(1.5),
        json!(366),
        json!("30"),
        json!({"days": 30}),
    ] {
        let mut database = Database::open_in_memory().unwrap();
        assert!(matches!(
            database.set_setting("application", "history.retention_days", value, 0),
            Err(DatabaseError::SettingNotAllowed)
        ));
    }

    let mut database = Database::open_in_memory().unwrap();
    let zero = database
        .set_setting("application", "history.retention_days", json!(0), 0)
        .unwrap();
    assert_eq!(zero.value, json!(0));

    let mut custom = Database::open_in_memory().unwrap();
    let setting = custom
        .set_setting("application", "history.retention_days", json!(45), 0)
        .unwrap();
    assert_eq!(setting.value, json!(45));
}

#[test]
fn runtime_retention_uses_the_configured_cutoff_and_preserves_active_records() {
    let mut database = Database::open_in_memory().unwrap();
    database
        .set_setting("application", "history.retention_days", json!(30), 0)
        .unwrap();

    let mut old = sample_job();
    old.id = "018f0f17-2f4a-7fb1-a247-303030303030".to_owned();
    old.state = JobState::Failed;
    old.stage = Some(OperationStage::Cleanup);
    old.updated_at = "2026-06-01T00:00:00Z".to_owned();
    old.finished_at = Some(old.updated_at.clone());
    database.create_job(&old).unwrap();

    let mut recent = sample_job();
    recent.id = "018f0f17-2f4a-7fb1-a247-404040404040".to_owned();
    recent.state = JobState::Failed;
    recent.stage = Some(OperationStage::Cleanup);
    recent.updated_at = "2026-08-10T00:00:00Z".to_owned();
    recent.finished_at = Some(recent.updated_at.clone());
    database.create_job(&recent).unwrap();

    let active = sample_job();
    database.create_job(&active).unwrap();
    let maintenance_time = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    assert_eq!(database.run_retention_at(maintenance_time).unwrap(), 1);
    assert!(database.get_job(&old.id).unwrap().is_none());
    assert!(database.get_job(&recent.id).unwrap().is_some());
    assert!(database.get_job(&active.id).unwrap().is_some());
}

#[test]
fn ambiguous_legacy_terminal_history_is_quarantined_from_all_deletion_paths() {
    let mut database = Database::open_in_memory().unwrap();
    let mut legacy = sample_job();
    legacy.id = "018f0f17-2f4a-7fb1-a247-505050505050".to_owned();
    legacy.operation_version = "1.0.0".to_owned();
    legacy.state = JobState::Failed;
    legacy.stage = Some(OperationStage::Cleanup);
    legacy.updated_at = "2026-06-01T00:00:00Z".to_owned();
    legacy.finished_at = Some(legacy.updated_at.clone());
    database.create_job(&legacy).unwrap();

    assert_eq!(
        database
            .purge_terminal_before("2026-08-16T00:00:00Z")
            .unwrap(),
        0
    );
    assert!(matches!(
        database.delete_terminal_history(&[legacy.id.clone()]),
        Err(DatabaseError::LegacyCleanupUnproven)
    ));
    assert!(database.get_job(&legacy.id).unwrap().is_some());
}

#[test]
fn zero_day_retention_purges_eligible_history_but_not_active_jobs() {
    let mut database = Database::open_in_memory().unwrap();
    database
        .set_setting("application", "history.retention_days", json!(0), 0)
        .unwrap();
    let mut terminal = sample_job();
    terminal.id = "018f0f17-2f4a-7fb1-a247-909090909090".to_owned();
    terminal.state = JobState::Failed;
    terminal.stage = Some(OperationStage::Cleanup);
    terminal.updated_at = "2026-08-16T11:59:59Z".to_owned();
    terminal.finished_at = Some(terminal.updated_at.clone());
    database.create_job(&terminal).unwrap();
    let active = sample_job();
    database.create_job(&active).unwrap();

    let maintenance_time = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    assert_eq!(database.run_retention_at(maintenance_time).unwrap(), 1);
    assert!(database.get_job(&terminal.id).unwrap().is_none());
    assert!(database.get_job(&active.id).unwrap().is_some());
}

#[test]
fn retention_maintenance_deletes_at_most_one_thousand_rows_per_run() {
    let mut database = Database::open_in_memory().unwrap();
    for index in 0..1001_u64 {
        let mut terminal = sample_job();
        terminal.id = format!("018f0f17-2f4a-7fb1-a247-{index:012x}");
        terminal.state = JobState::Failed;
        terminal.stage = Some(OperationStage::Cleanup);
        terminal.updated_at = "2026-06-01T00:00:00Z".to_owned();
        terminal.finished_at = Some(terminal.updated_at.clone());
        database.create_job(&terminal).unwrap();
    }

    assert_eq!(
        database
            .purge_terminal_before("2026-08-16T00:00:00Z")
            .unwrap(),
        1000
    );
    let remaining: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn publication_reservation_activation_and_release_are_exact() {
    let mut database = Database::open_in_memory().unwrap();
    let job = sample_job();
    database.create_job(&job).unwrap();
    database
        .connection()
        .execute(
            "UPDATE jobs SET state = 'verifying' WHERE id = ?1",
            [&job.id],
        )
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE job_outputs SET status = 'verified' WHERE job_id = ?1",
            [&job.id],
        )
        .unwrap();
    let final_path = r"C:\Users\Example\Documents\sample-copy.txt";
    let partial_path = format!(
        r"C:\Users\Example\Documents\.document-studio-{}-11111111-1111-4111-8111-111111111111.partial",
        job.id
    );
    database
        .reserve_publication_attempt(
            &job.id,
            "sample-copy.txt",
            final_path,
            &partial_path,
            16,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
    let reserved = database.get_job(&job.id).unwrap().unwrap();
    assert_eq!(reserved.outputs[0].final_path.as_deref(), Some(final_path));
    assert_eq!(
        reserved.outputs[0].partial_path.as_deref(),
        Some(partial_path.as_str())
    );
    let ownership_result_code = concat!(
        "DESTINATION_PARTIAL_OWNED:11111111-1111-4111-8111-111111111111:",
        "volume-00000001:file-0000000000000002"
    );
    assert!(!database
        .owned_partial_is_activated(&job.id, &partial_path, ownership_result_code)
        .unwrap());
    database
        .activate_owned_partial(&job.id, &partial_path, ownership_result_code)
        .unwrap();
    assert!(database
        .owned_partial_is_activated(&job.id, &partial_path, ownership_result_code)
        .unwrap());

    assert!(matches!(
        database.clear_owned_partial(&job.id, r"C:\wrong.partial"),
        Err(DatabaseError::JobConflict)
    ));
    assert_eq!(
        database.get_job(&job.id).unwrap().unwrap().outputs[0]
            .partial_path
            .as_deref(),
        Some(partial_path.as_str())
    );
    database
        .clear_owned_partial(&job.id, &partial_path)
        .unwrap();
    assert!(database.get_job(&job.id).unwrap().unwrap().outputs[0]
        .partial_path
        .is_none());
}
