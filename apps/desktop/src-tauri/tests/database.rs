use std::fs;

use chrono::{TimeZone, Utc};
use document_studio_lib::contracts::{
    CorePdfPlanPayload, JobRecord, JobState, OperationPlanEnvelope, OperationSpecEnvelope,
    OperationStage, OutputRotation, OutputStatus, PageRotation, RotatePagesPlan, SplitOutputRange,
    SplitPlan, StoredOperationPlan, StoredOperationSpec, CORE_PDF_OPERATION_VERSION,
    IMAGE_TO_PDF_OPERATION_ID, IMAGE_TO_PDF_VERSION, PDF_EXTRACT_OPERATION_ID,
    PDF_ROTATE_OPERATION_ID, PDF_SPLIT_OPERATION_ID,
};
use document_studio_lib::database::{
    apply_migrations, configure_connection, Database, DatabaseError, Migration,
    FOUNDATION_MIGRATIONS,
};
use document_studio_lib::page_plan::validate_plan;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const GOLDEN: &str =
    include_str!("../../../../packages/contracts/fixtures/foundation-contracts.json");

fn sample_job() -> JobRecord {
    let fixture: Value = serde_json::from_str(GOLDEN).unwrap();
    serde_json::from_value(fixture["job"].clone()).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sample_split_job(id: &str) -> (JobRecord, StoredOperationPlan) {
    let mut job = sample_job();
    job.id = id.to_owned();
    job.operation_id = PDF_SPLIT_OPERATION_ID.to_owned();
    job.operation_version = CORE_PDF_OPERATION_VERSION.to_owned();
    job.requested_output_name = "part-001.pdf".to_owned();
    job.outputs[0].requested_name = "part-001.pdf".to_owned();
    job.outputs[0].mime_type = "application/pdf".to_owned();
    job.outputs[0].status = OutputStatus::Planned;
    let mut second = job.outputs[0].clone();
    second.ordinal = 1;
    second.requested_name = "part-002.pdf".to_owned();
    job.outputs.push(second);
    let validated = validate_plan(OperationPlanEnvelope {
        schema_version: 1,
        operation_id: PDF_SPLIT_OPERATION_ID.to_owned(),
        source_page_count: 2,
        payload: CorePdfPlanPayload::Split(SplitPlan {
            ranges: vec![
                SplitOutputRange {
                    start_page_index: 0,
                    end_page_index: 0,
                    output_name: "part-001.pdf".to_owned(),
                },
                SplitOutputRange {
                    start_page_index: 1,
                    end_page_index: 1,
                    output_name: "part-002.pdf".to_owned(),
                },
            ],
        }),
    })
    .unwrap();
    (job, validated.stored)
}

fn insert_version_three_job(connection: &Connection, job: &JobRecord) {
    connection
        .execute(
            "INSERT INTO jobs (
                id, operation_id, operation_version, state, stage, sequence,
                completed_units, total_units, unit, destination_directory,
                requested_output_name, resolved_output_name, cancellation_requested_at,
                created_at, updated_at, finished_at, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                job.id,
                job.operation_id,
                job.operation_version,
                job.state.as_str(),
                job.stage.map(OperationStage::as_str),
                i64::try_from(job.sequence).unwrap(),
                i64::try_from(job.progress.completed_units).unwrap(),
                i64::try_from(job.progress.total_units).unwrap(),
                job.progress.unit.as_str(),
                job.destination_directory,
                job.requested_output_name,
                job.resolved_output_name,
                job.cancellation_requested_at,
                job.created_at,
                job.updated_at,
                job.finished_at,
                i64::try_from(job.version).unwrap(),
            ],
        )
        .unwrap();
    for input in &job.inputs {
        connection
            .execute(
                "INSERT INTO job_inputs (
                    job_id, ordinal, display_name, source_path, canonical_path, file_identity,
                    size_bytes, modified_at, mime_type, sha256, password_reference
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    job.id,
                    input.ordinal,
                    input.display_name,
                    input.source_path,
                    input.canonical_path,
                    input.file_identity,
                    i64::try_from(input.size_bytes).unwrap(),
                    input.modified_at,
                    input.mime_type,
                    input.sha256,
                    input.password_reference,
                ],
            )
            .unwrap();
    }
    for output in &job.outputs {
        connection
            .execute(
                "INSERT INTO job_outputs (
                    job_id, ordinal, requested_name, resolved_name, staging_path, partial_path,
                    final_path, size_bytes, mime_type, sha256, status, verified_at, published_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    job.id,
                    output.ordinal,
                    output.requested_name,
                    output.resolved_name,
                    output.staging_path,
                    output.partial_path,
                    output.final_path,
                    output.size_bytes.map(|size| i64::try_from(size).unwrap()),
                    output.mime_type,
                    output.sha256,
                    output.status.as_str(),
                    output.verified_at,
                    output.published_at,
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO job_stage_runs (
                job_id, sequence, stage, started_at, finished_at,
                completed_units, total_units, safe_result_code
             ) VALUES (?1, 0, 'inspect', ?2, ?2, 1, 1, 'LEGACY_INSPECTED')",
            params![job.id, job.created_at],
        )
        .unwrap();
}

#[test]
fn fresh_database_migrates_to_version_five_and_reopens_idempotently() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("metadata.sqlite3");
    {
        let database = Database::open(&path).unwrap();
        assert_eq!(database.migration_versions().unwrap(), vec![1, 2, 3, 4, 5]);
        let integrity: String = database
            .connection()
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.migration_versions().unwrap(), vec![1, 2, 3, 4, 5]);
}

#[test]
fn migrations_one_through_three_upgrade_to_five_preserves_legacy_jobs_without_backfill() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("version-three.sqlite3");
    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&connection).unwrap();
    apply_migrations(&mut connection, &FOUNDATION_MIGRATIONS[..3]).unwrap();

    let checksums_before = connection
        .prepare("SELECT version, sql_checksum FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(checksums_before.len(), 3);

    let diagnostic = sample_job();
    let mut merge = sample_job();
    merge.id = "018f0f17-2f4a-7fb1-a247-303030303030".to_owned();
    merge.operation_id = "pdf.merge".to_owned();
    merge.operation_version = "1.0.0".to_owned();
    merge.requested_output_name = "merged.pdf".to_owned();
    merge.inputs[0].display_name = "first.pdf".to_owned();
    merge.inputs[0].mime_type = "application/pdf".to_owned();
    let mut second_input = merge.inputs[0].clone();
    second_input.ordinal = 1;
    second_input.display_name = "second.pdf".to_owned();
    second_input.file_identity = "volume-1:file-84".to_owned();
    merge.inputs.push(second_input);
    merge.outputs[0].requested_name = "merged.pdf".to_owned();
    merge.outputs[0].mime_type = "application/pdf".to_owned();
    insert_version_three_job(&connection, &diagnostic);
    insert_version_three_job(&connection, &merge);
    connection
        .execute(
            "INSERT INTO settings(scope, key, value_json, version, updated_at)
             VALUES ('application', 'history.retention_days', '30', 7, ?1)",
            [&diagnostic.created_at],
        )
        .unwrap();
    drop(connection);

    let database = Database::open(&path).unwrap();
    assert_eq!(database.migration_versions().unwrap(), vec![1, 2, 3, 4, 5]);
    let checksums_after = database
        .connection()
        .prepare(
            "SELECT version, sql_checksum FROM schema_migrations WHERE version <= 3 ORDER BY version",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(checksums_after, checksums_before);
    let migration_four_count: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 4",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migration_four_count, 1);
    let migration_five_count: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migration_five_count, 1);
    assert_eq!(
        database.get_job(&diagnostic.id).unwrap(),
        Some(diagnostic.clone())
    );
    assert_eq!(database.get_job(&merge.id).unwrap(), Some(merge.clone()));
    assert!(
        database
            .get_operation_plan(&diagnostic.id)
            .unwrap()
            .is_none(),
        "migration 4 must not backfill diagnostic.copy with an invented plan"
    );
    assert!(
        database.get_operation_plan(&merge.id).unwrap().is_none(),
        "migration 4 must not backfill pdf.merge with an invented plan"
    );
    let plan_count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM job_operation_plans", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        plan_count, 0,
        "migration 4 must add no default or empty plans"
    );
    let spec_count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM job_operation_specs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        spec_count, 0,
        "migration 5 must not invent operation settings"
    );
    let warning_count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM job_warnings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(warning_count, 0, "migration 5 must not invent warnings");
    let preserved_stage_runs: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM job_stage_runs
             WHERE safe_result_code = 'LEGACY_INSPECTED'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved_stage_runs, 2);
    let setting: (String, i64) = database
        .connection()
        .query_row(
            "SELECT value_json, version FROM settings
             WHERE scope = 'application' AND key = 'history.retention_days'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(setting, ("30".to_owned(), 7));
    let foreign_key_failures = database
        .connection()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(foreign_key_failures, 0);
    for index in [
        "jobs_state_updated_idx",
        "jobs_operation_created_idx",
        "job_errors_job_created_idx",
        "workflow_runs_status_idx",
        "job_operation_plans_operation_idx",
        "job_operation_specs_operation_idx",
        "job_warnings_job_created_idx",
    ] {
        let present: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            present, 1,
            "expected index {index} must survive the upgrade"
        );
    }
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
    for (table, _sql) in &schemas {
        let mut columns = database
            .connection()
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .unwrap();
        let types = columns
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(types.iter().all(|column_type| column_type != "BLOB"));
    }

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
fn typed_operation_plan_is_canonical_hashed_bounded_and_multi_output() {
    let mut database = Database::open_in_memory().unwrap();
    let mut job = sample_job();
    job.operation_id = PDF_SPLIT_OPERATION_ID.to_owned();
    job.operation_version = CORE_PDF_OPERATION_VERSION.to_owned();
    job.requested_output_name = "part-001.pdf".to_owned();
    job.outputs[0].requested_name = "part-001.pdf".to_owned();
    job.outputs[0].status = OutputStatus::Planned;
    let mut second = job.outputs[0].clone();
    second.ordinal = 1;
    second.requested_name = "part-002.pdf".to_owned();
    job.outputs.push(second);
    let validated = validate_plan(OperationPlanEnvelope {
        schema_version: 1,
        operation_id: PDF_SPLIT_OPERATION_ID.to_owned(),
        source_page_count: 2,
        payload: CorePdfPlanPayload::Split(SplitPlan {
            ranges: vec![
                SplitOutputRange {
                    start_page_index: 0,
                    end_page_index: 0,
                    output_name: "part-001.pdf".to_owned(),
                },
                SplitOutputRange {
                    start_page_index: 1,
                    end_page_index: 1,
                    output_name: "part-002.pdf".to_owned(),
                },
            ],
        }),
    })
    .unwrap();
    database
        .create_job_with_plan(&job, &validated.stored)
        .unwrap();

    let stored = database.get_operation_plan(&job.id).unwrap().unwrap();
    assert_eq!(stored, validated.stored);
    assert_eq!(database.get_job(&job.id).unwrap().unwrap().outputs.len(), 2);

    let oversized = "x".repeat(65_537);
    assert!(database
        .connection()
        .execute(
            "UPDATE job_operation_plans SET plan_json = ?1 WHERE job_id = ?2",
            (&oversized, &job.id),
        )
        .is_err());
    let schema: String = database
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'job_operation_plans'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(schema.contains("length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536"));

    let mut tampered = validated.stored;
    tampered.sha256 = "0".repeat(64);
    let mut rejected = job;
    rejected.id = "018f0f17-2f4a-7fb1-a247-202020202020".to_owned();
    assert!(matches!(
        database.create_job_with_plan(&rejected, &tampered),
        Err(DatabaseError::OperationPlanInvalid)
    ));
    assert!(database.get_job(&rejected.id).unwrap().is_none());
}

#[test]
fn operation_specs_are_canonical_hashed_bounded_and_warnings_are_sanitized() {
    let mut database = Database::open_in_memory().unwrap();
    let mut job = sample_job();
    job.operation_id = IMAGE_TO_PDF_OPERATION_ID.to_owned();
    job.operation_version = IMAGE_TO_PDF_VERSION.to_owned();
    job.requested_output_name = "images.pdf".to_owned();
    job.inputs[0].display_name = "image.png".to_owned();
    job.inputs[0].mime_type = "image/png".to_owned();
    job.outputs[0].requested_name = "images.pdf".to_owned();
    job.outputs[0].mime_type = "application/pdf".to_owned();
    job.outputs[0].status = OutputStatus::Planned;
    let envelope = OperationSpecEnvelope {
        schema_version: 1,
        operation_id: IMAGE_TO_PDF_OPERATION_ID.to_owned(),
        settings: json!({"alphaPolicy":"preserve-soft-mask","pageSizing":"one-point-per-oriented-pixel"}),
    };
    let canonical_json = serde_json::to_string(&envelope).unwrap();
    let spec = StoredOperationSpec {
        envelope,
        sha256: sha256_hex(canonical_json.as_bytes()),
        canonical_json,
        created_at: job.created_at.clone(),
    };
    database.create_job_with_spec(&job, &spec).unwrap();
    assert_eq!(database.get_operation_spec(&job.id).unwrap(), Some(spec));
    database
        .record_warning(
            &job.id,
            "ICC_PROFILE_NOT_RETAINED",
            "The embedded color profile was normalized.",
            Some(0),
            Some(0),
        )
        .unwrap();
    let warnings = database.list_warnings(&job.id).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].input_index, Some(0));
    assert_eq!(warnings[0].page_index, Some(0));
    assert!(!warnings[0].sanitized_detail.contains(r"C:\"));

    assert!(database
        .connection()
        .execute(
            "UPDATE job_operation_specs SET settings_json = ?1 WHERE job_id = ?2",
            ("x".repeat(65_537), &job.id),
        )
        .is_err());
    assert!(database
        .record_warning(&job.id, "X", "short code", None, None)
        .is_err());
    assert!(database
        .record_warning(&job.id, "DETAIL_TOO_LONG", &"x".repeat(501), None, None)
        .is_err());
}

#[test]
fn invalid_plan_json_is_rejected_by_sql_and_typed_repository_loading() {
    let mut database = Database::open_in_memory().unwrap();
    let (job, _plan) = sample_split_job("018f0f17-2f4a-7fb1-a247-404040404040");
    database.create_job(&job).unwrap();
    let before = database.get_job(&job.id).unwrap().unwrap();
    let before_job_count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
        .unwrap();
    let invalid = database.connection().execute(
        "INSERT INTO job_operation_plans (
            job_id, schema_version, operation_id, source_page_count,
            plan_json, plan_sha256, created_at
         ) VALUES (?1, 1, ?2, 2, '{', ?3, ?4)",
        params![
            job.id,
            PDF_SPLIT_OPERATION_ID,
            "0".repeat(64),
            job.created_at
        ],
    );
    assert!(
        invalid.is_err(),
        "json_valid must reject malformed plan JSON"
    );
    let plan_count: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM job_operation_plans WHERE job_id = ?1",
            [&job.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(plan_count, 0);
    assert_eq!(database.get_job(&job.id).unwrap(), Some(before));
    let after_job_count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after_job_count, before_job_count);

    let structurally_invalid = "{}";
    database
        .connection()
        .execute(
            "INSERT INTO job_operation_plans (
                job_id, schema_version, operation_id, source_page_count,
                plan_json, plan_sha256, created_at
             ) VALUES (?1, 1, ?2, 2, ?3, ?4, ?5)",
            params![
                job.id,
                PDF_SPLIT_OPERATION_ID,
                structurally_invalid,
                sha256_hex(structurally_invalid.as_bytes()),
                job.created_at,
            ],
        )
        .unwrap();
    assert!(matches!(
        database.get_operation_plan(&job.id),
        Err(DatabaseError::Json(_))
    ));
}

#[test]
fn job_and_plan_operation_mismatch_rolls_back_without_leaking_plan_details() {
    let mut database = Database::open_in_memory().unwrap();
    let mut job = sample_job();
    job.id = "018f0f17-2f4a-7fb1-a247-505050505050".to_owned();
    job.operation_id = PDF_EXTRACT_OPERATION_ID.to_owned();
    job.operation_version = CORE_PDF_OPERATION_VERSION.to_owned();
    job.requested_output_name = "extract.pdf".to_owned();
    job.outputs[0].requested_name = "extract.pdf".to_owned();
    job.outputs[0].mime_type = "application/pdf".to_owned();
    let rotate = validate_plan(OperationPlanEnvelope {
        schema_version: 1,
        operation_id: PDF_ROTATE_OPERATION_ID.to_owned(),
        source_page_count: 1,
        payload: CorePdfPlanPayload::Rotate(RotatePagesPlan {
            rotations: vec![PageRotation {
                page_index: 0,
                clockwise_degrees: OutputRotation::Clockwise90,
            }],
            output_name: "rotate.pdf".to_owned(),
        }),
    })
    .unwrap();
    let error = database
        .create_job_with_plan(&job, &rotate.stored)
        .unwrap_err();
    assert!(matches!(error, DatabaseError::OperationPlanInvalid));
    assert_eq!(
        error.to_string(),
        "operation plan is not canonical, bounded, or consistent with the job"
    );
    assert!(!error.to_string().contains(&job.destination_directory));
    assert!(!error.to_string().contains(PDF_EXTRACT_OPERATION_ID));
    assert!(!error.to_string().contains(PDF_ROTATE_OPERATION_ID));
    for table in ["jobs", "job_inputs", "job_outputs", "job_operation_plans"] {
        let count: i64 = database
            .connection()
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE job_id = ?1"),
                [&job.id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| {
                database
                    .connection()
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                        [&job.id],
                        |row| row.get(0),
                    )
                    .unwrap()
            });
        assert_eq!(count, 0, "{table} must roll back with the rejected plan");
    }

    let (stored_job, stored_plan) = sample_split_job("018f0f17-2f4a-7fb1-a247-606060606060");
    database
        .create_job_with_plan(&stored_job, &stored_plan)
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE job_operation_plans SET operation_id = ?1 WHERE job_id = ?2",
            params![PDF_ROTATE_OPERATION_ID, stored_job.id],
        )
        .unwrap();
    let tampered_plan_result = database.get_operation_plan(&stored_job.id);
    assert!(
        matches!(
            tampered_plan_result,
            Err(DatabaseError::OperationPlanMismatch)
        ),
        "unexpected tampered-plan result: {tampered_plan_result:?}"
    );

    database
        .connection()
        .execute(
            "UPDATE job_operation_plans SET operation_id = ?1 WHERE job_id = ?2",
            params![stored_plan.envelope.operation_id, stored_job.id],
        )
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE jobs SET operation_id = ?1 WHERE id = ?2",
            params![PDF_ROTATE_OPERATION_ID, stored_job.id],
        )
        .unwrap();
    assert!(matches!(
        database.get_operation_plan(&stored_job.id),
        Err(DatabaseError::OperationPlanMismatch)
    ));
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
fn input_hash_updates_address_the_exact_persisted_ordinal() {
    let mut database = Database::open_in_memory().unwrap();
    let mut job = sample_job();
    let mut second = job.inputs[0].clone();
    second.ordinal = 1;
    second.display_name = "second.pdf".to_owned();
    second.source_path = r"C:\Users\Example\Documents\second.pdf".to_owned();
    second.canonical_path = second.source_path.clone();
    second.file_identity = "volume-1:file-43".to_owned();
    job.inputs.push(second);
    database.create_job(&job).unwrap();

    let first_hash = "1".repeat(64);
    let second_hash = "2".repeat(64);
    database
        .update_input_hash(&job.id, 1, &second_hash)
        .unwrap();
    database.update_input_hash(&job.id, 0, &first_hash).unwrap();

    let stored = database.get_job(&job.id).unwrap().unwrap();
    assert_eq!(
        stored.inputs[0].sha256.as_deref(),
        Some(first_hash.as_str())
    );
    assert_eq!(
        stored.inputs[1].sha256.as_deref(),
        Some(second_hash.as_str())
    );
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
