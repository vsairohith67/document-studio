use std::fs;

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
