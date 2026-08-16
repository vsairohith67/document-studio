use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contracts::{
    DependencyDiagnostic, JobInput, JobOutput, JobProgress, JobRecord, JobState, OperationError,
    OperationStage, OutputStatus, ProgressUnit, SettingRecord, DEFAULT_HISTORY_RETENTION_DAYS,
    DIAGNOSTIC_COPY_OPERATION_ID, DIAGNOSTIC_COPY_VERSION, HISTORY_RETENTION_KEY,
    HISTORY_RETENTION_SCOPE, LEGACY_CLEANUP_PROVEN, LEGACY_CLEANUP_UNPROVEN,
    LEGACY_DIAGNOSTIC_COPY_VERSION, MAX_HISTORY_PURGE,
};
use crate::job_engine::can_transition;

const MIGRATION_LEDGER_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    name TEXT NOT NULL UNIQUE,
    sql_checksum TEXT NOT NULL CHECK (length(sql_checksum) = 64),
    applied_at TEXT NOT NULL
) STRICT;
"#;

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const FOUNDATION_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "metadata",
        sql: include_str!("../migrations/0001_metadata.sql"),
    },
    Migration {
        version: 2,
        name: "jobs",
        sql: include_str!("../migrations/0002_jobs.sql"),
    },
    Migration {
        version: 3,
        name: "workflows",
        sql: include_str!("../migrations/0003_workflows.sql"),
    },
];

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("metadata database error")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration {version} checksum does not match the recorded schema")]
    MigrationChecksum { version: i64 },
    #[error("database contains unknown migration {version}")]
    UnknownMigration { version: i64 },
    #[error("invalid stored contract value for {field}")]
    InvalidContractValue { field: &'static str },
    #[error("setting is not allowed in the foundation")]
    SettingNotAllowed,
    #[error("setting update conflicted with a newer version")]
    SettingConflict,
    #[error("job state or version changed before the update")]
    JobConflict,
    #[error("illegal job state transition")]
    IllegalTransition,
    #[error("terminal state requires publication and cleanup evidence")]
    TerminalEvidenceMissing,
    #[error("legacy destination cleanup cannot be proven")]
    LegacyCleanupUnproven,
    #[error("JSON value is not valid for metadata storage")]
    Json(#[from] serde_json::Error),
}

pub struct Database {
    connection: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationEvidence {
    pub final_path: String,
    pub resolved_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub status: OutputStatus,
    pub partial_path: Option<String>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    pub fn from_connection(mut connection: Connection) -> Result<Self, DatabaseError> {
        configure_connection(&connection)?;
        apply_migrations(&mut connection, FOUNDATION_MIGRATIONS)?;
        Ok(Self { connection })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    pub fn migration_versions(&self) -> Result<Vec<i64>, DatabaseError> {
        let mut statement = self
            .connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_job(&mut self, job: &JobRecord) -> Result<(), DatabaseError> {
        let sequence = metadata_i64(job.sequence, "job sequence")?;
        let completed_units = metadata_i64(job.progress.completed_units, "completed units")?;
        let total_units = metadata_i64(job.progress.total_units, "total units")?;
        let version = metadata_i64(job.version, "job version")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
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
                sequence,
                completed_units,
                total_units,
                job.progress.unit.as_str(),
                job.destination_directory,
                job.requested_output_name,
                job.resolved_output_name,
                job.cancellation_requested_at,
                job.created_at,
                job.updated_at,
                job.finished_at,
                version,
            ],
        )?;

        for input in &job.inputs {
            let size_bytes = metadata_i64(input.size_bytes, "input size")?;
            transaction.execute(
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
                    size_bytes,
                    input.modified_at,
                    input.mime_type,
                    input.sha256,
                    input.password_reference,
                ],
            )?;
        }

        for output in &job.outputs {
            let size_bytes = output
                .size_bytes
                .map(|value| metadata_i64(value, "output size"))
                .transpose()?;
            transaction.execute(
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
                    size_bytes,
                    output.mime_type,
                    output.sha256,
                    output.status.as_str(),
                    output.verified_at,
                    output.published_at,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Option<JobRecord>, DatabaseError> {
        let header = self
            .connection
            .query_row(
                "SELECT operation_id, operation_version, state, stage, sequence,
                        completed_units, total_units, unit, destination_directory,
                        requested_output_name, resolved_output_name, cancellation_requested_at,
                        created_at, updated_at, finished_at, version
                 FROM jobs WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row_u64(row, 4)?,
                        row_u64(row, 5)?,
                        row_u64(row, 6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row_u64(row, 15)?,
                    ))
                },
            )
            .optional()?;

        let Some((
            operation_id,
            operation_version,
            state,
            stage,
            sequence,
            completed_units,
            total_units,
            unit,
            destination_directory,
            requested_output_name,
            resolved_output_name,
            cancellation_requested_at,
            created_at,
            updated_at,
            finished_at,
            version,
        )) = header
        else {
            return Ok(None);
        };

        let inputs = self.load_inputs(id)?;
        let outputs = self.load_outputs(id)?;
        let errors = self.load_errors(id)?;
        let stage = match stage {
            Some(value) => Some(
                OperationStage::from_contract(&value)
                    .ok_or(DatabaseError::InvalidContractValue { field: "job stage" })?,
            ),
            None => None,
        };

        Ok(Some(JobRecord {
            id: id.to_owned(),
            operation_id,
            operation_version,
            state: JobState::from_contract(&state)
                .ok_or(DatabaseError::InvalidContractValue { field: "job state" })?,
            stage,
            sequence,
            progress: JobProgress {
                completed_units,
                total_units,
                unit: ProgressUnit::from_contract(&unit).ok_or(
                    DatabaseError::InvalidContractValue {
                        field: "progress unit",
                    },
                )?,
            },
            destination_directory,
            requested_output_name,
            resolved_output_name,
            cancellation_requested_at,
            created_at,
            updated_at,
            finished_at,
            version,
            inputs,
            outputs,
            errors,
        }))
    }

    pub fn list_jobs(&self, limit: u32) -> Result<Vec<JobRecord>, DatabaseError> {
        let ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM jobs ORDER BY updated_at DESC LIMIT ?1")?;
            let rows = statement.query_map([limit.min(200)], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        ids.iter()
            .map(|id| {
                self.get_job(id)?
                    .ok_or(DatabaseError::InvalidContractValue {
                        field: "job reference",
                    })
            })
            .collect()
    }

    pub fn transition_job(
        &mut self,
        id: &str,
        expected_state: JobState,
        expected_version: u64,
        next: JobState,
        stage: Option<OperationStage>,
    ) -> Result<u64, DatabaseError> {
        if !can_transition(expected_state, next) {
            return Err(DatabaseError::IllegalTransition);
        }
        if next.is_terminal() && !self.terminal_evidence_is_ready(id, next)? {
            return Err(DatabaseError::TerminalEvidenceMissing);
        }

        let updated_at = now();
        let finished_at = next.is_terminal().then_some(updated_at.as_str());
        let expected_version_i64 = metadata_i64(expected_version, "job version")?;
        let changed = self.connection.execute(
            "UPDATE jobs
             SET state = ?1, stage = ?2, updated_at = ?3, finished_at = ?4,
                 version = version + 1, sequence = sequence + 1
             WHERE id = ?5 AND state = ?6 AND version = ?7",
            params![
                next.as_str(),
                stage.map(OperationStage::as_str),
                updated_at,
                finished_at,
                id,
                expected_state.as_str(),
                expected_version_i64,
            ],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(expected_version + 1)
    }

    pub fn request_cancellation(&mut self, id: &str) -> Result<bool, DatabaseError> {
        let changed = self.connection.execute(
            "UPDATE jobs SET cancellation_requested_at = ?1, updated_at = ?1, version = version + 1
             WHERE id = ?2 AND state IN ('queued', 'inspecting', 'preflight', 'ready', 'running', 'verifying')
               AND cancellation_requested_at IS NULL",
            params![now(), id],
        )?;
        Ok(changed == 1)
    }

    pub fn update_progress(
        &mut self,
        id: &str,
        expected_state: JobState,
        stage: OperationStage,
        completed_units: u64,
        total_units: u64,
        unit: ProgressUnit,
    ) -> Result<u64, DatabaseError> {
        let completed_units = metadata_i64(completed_units, "completed units")?;
        let total_units = metadata_i64(total_units, "total units")?;
        let changed = self.connection.execute(
            "UPDATE jobs
             SET stage = ?1, completed_units = ?2, total_units = ?3, unit = ?4,
                 sequence = sequence + 1, version = version + 1, updated_at = ?5
             WHERE id = ?6 AND state = ?7",
            params![
                stage.as_str(),
                completed_units,
                total_units,
                unit.as_str(),
                now(),
                id,
                expected_state.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        let sequence =
            self.connection
                .query_row("SELECT sequence FROM jobs WHERE id = ?1", [id], |row| {
                    row_u64(row, 0)
                })?;
        Ok(sequence)
    }

    pub fn update_input_hash(&mut self, id: &str, sha256: &str) -> Result<(), DatabaseError> {
        let changed = self.connection.execute(
            "UPDATE job_inputs SET sha256 = ?1 WHERE job_id = ?2 AND ordinal = 0",
            params![sha256, id],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn set_output_staging(
        &mut self,
        id: &str,
        staging_path: &str,
        size_bytes: u64,
        sha256: &str,
        verified_at: &str,
    ) -> Result<(), DatabaseError> {
        let size_bytes = metadata_i64(size_bytes, "output size")?;
        let changed = self.connection.execute(
            "UPDATE job_outputs
             SET staging_path = ?1, size_bytes = ?2, sha256 = ?3,
                 status = 'verified', verified_at = ?4
             WHERE job_id = ?5 AND ordinal = 0",
            params![staging_path, size_bytes, sha256, verified_at, id],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn reserve_publication_attempt(
        &mut self,
        id: &str,
        resolved_name: &str,
        final_path: &str,
        partial_path: &str,
        size_bytes: u64,
        sha256: &str,
    ) -> Result<(), DatabaseError> {
        let size_bytes = metadata_i64(size_bytes, "output size")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let output_changed = transaction.execute(
            "UPDATE job_outputs
             SET resolved_name = ?1, final_path = ?2, partial_path = ?3,
                 size_bytes = ?4, sha256 = ?5
             WHERE job_id = ?6 AND ordinal = 0 AND partial_path IS NULL
               AND status IN ('verified', 'publishing')",
            params![
                resolved_name,
                final_path,
                partial_path,
                size_bytes,
                sha256,
                id
            ],
        )?;
        let job_changed = transaction.execute(
            "UPDATE jobs
             SET resolved_output_name = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND state IN ('verifying', 'publishing')",
            params![resolved_name, now(), id],
        )?;
        if output_changed != 1 || job_changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn clear_owned_partial(
        &mut self,
        id: &str,
        expected_partial_path: &str,
    ) -> Result<(), DatabaseError> {
        let changed = self.connection.execute(
            "UPDATE job_outputs SET partial_path = NULL
             WHERE job_id = ?1 AND ordinal = 0 AND partial_path = ?2",
            params![id, expected_partial_path],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn activate_owned_partial(
        &mut self,
        id: &str,
        expected_partial_path: &str,
        ownership_result_code: &str,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reserved: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM jobs
                JOIN job_outputs ON job_outputs.job_id = jobs.id AND job_outputs.ordinal = 0
                WHERE jobs.id = ?1 AND jobs.operation_id = ?2 AND jobs.operation_version = ?3
                  AND jobs.state IN ('verifying', 'publishing')
                  AND job_outputs.partial_path = ?4
             )",
            params![
                id,
                DIAGNOSTIC_COPY_OPERATION_ID,
                DIAGNOSTIC_COPY_VERSION,
                expected_partial_path,
            ],
            |row| row.get(0),
        )?;
        if !reserved {
            return Err(DatabaseError::JobConflict);
        }
        let already_activated: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM job_stage_runs
                WHERE job_id = ?1 AND stage = 'publish' AND safe_result_code = ?2
             )",
            params![id, ownership_result_code],
            |row| row.get(0),
        )?;
        if !already_activated {
            let sequence: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(sequence), -1) + 1 FROM job_stage_runs WHERE job_id = ?1",
                [id],
                |row| row.get(0),
            )?;
            let activated_at = now();
            transaction.execute(
                "INSERT INTO job_stage_runs (
                    job_id, sequence, stage, started_at, finished_at,
                    completed_units, total_units, safe_result_code
                 ) VALUES (?1, ?2, 'publish', ?3, ?3, 0, 0, ?4)",
                params![id, sequence, activated_at, ownership_result_code],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn owned_partial_is_activated(
        &self,
        id: &str,
        expected_partial_path: &str,
        ownership_result_code: &str,
    ) -> Result<bool, DatabaseError> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM jobs
                    JOIN job_outputs ON job_outputs.job_id = jobs.id AND job_outputs.ordinal = 0
                    JOIN job_stage_runs ON job_stage_runs.job_id = jobs.id
                    WHERE jobs.id = ?1 AND jobs.operation_id = ?2 AND jobs.operation_version = ?3
                      AND job_outputs.partial_path = ?4
                      AND job_stage_runs.stage = 'publish'
                      AND job_stage_runs.safe_result_code = ?5
                 )",
                params![
                    id,
                    DIAGNOSTIC_COPY_OPERATION_ID,
                    DIAGNOSTIC_COPY_VERSION,
                    expected_partial_path,
                    ownership_result_code,
                ],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn clear_staging_path(
        &mut self,
        id: &str,
        expected_staging_path: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let changed = self.connection.execute(
            "UPDATE job_outputs SET staging_path = NULL
             WHERE job_id = ?1 AND ordinal = 0
               AND ((?2 IS NULL AND staging_path IS NULL) OR staging_path = ?2)",
            params![id, expected_staging_path],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn clear_unpublished_intent(&mut self, id: &str) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let output_changed = transaction.execute(
            "UPDATE job_outputs
             SET resolved_name = NULL, final_path = NULL,
                 status = CASE WHEN status = 'publishing' THEN 'verified' ELSE status END
             WHERE job_id = ?1 AND ordinal = 0 AND partial_path IS NULL
               AND status IN ('planned', 'staged', 'verified', 'publishing')",
            [id],
        )?;
        let job_changed = transaction.execute(
            "UPDATE jobs SET resolved_output_name = NULL, updated_at = ?1, version = version + 1
             WHERE id = ?2 AND state IN (
                'queued', 'inspecting', 'preflight', 'ready', 'running', 'verifying', 'interrupted'
             )",
            params![now(), id],
        )?;
        if output_changed != 1 || job_changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn set_publication_intent(
        &mut self,
        id: &str,
        resolved_name: &str,
        final_path: &str,
        size_bytes: u64,
        sha256: &str,
    ) -> Result<(), DatabaseError> {
        let size_bytes = metadata_i64(size_bytes, "output size")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE job_outputs
             SET resolved_name = ?1, final_path = ?2, size_bytes = ?3, sha256 = ?4,
                 status = 'publishing'
             WHERE job_id = ?5 AND ordinal = 0 AND partial_path IS NOT NULL
               AND resolved_name = ?1 AND final_path = ?2",
            params![resolved_name, final_path, size_bytes, sha256, id],
        )?;
        let state_changed = transaction.execute(
            "UPDATE jobs SET resolved_output_name = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND state = 'publishing'",
            params![resolved_name, now(), id],
        )?;
        if changed != 1 || state_changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn begin_publication(
        &mut self,
        id: &str,
        resolved_name: &str,
        final_path: &str,
        size_bytes: u64,
        sha256: &str,
    ) -> Result<(), DatabaseError> {
        let size_bytes = metadata_i64(size_bytes, "output size")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_version: i64 = transaction
            .query_row(
                "SELECT version FROM jobs WHERE id = ?1 AND state = 'verifying'",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::JobConflict)?;
        let output_changed = transaction.execute(
            "UPDATE job_outputs
             SET resolved_name = ?1, final_path = ?2, size_bytes = ?3, sha256 = ?4,
                 status = 'publishing'
             WHERE job_id = ?5 AND ordinal = 0 AND status = 'verified'
               AND partial_path IS NOT NULL AND resolved_name = ?1 AND final_path = ?2",
            params![resolved_name, final_path, size_bytes, sha256, id],
        )?;
        let state_changed = transaction.execute(
            "UPDATE jobs
             SET state = 'publishing', stage = 'publish', resolved_output_name = ?1,
                 updated_at = ?2, version = version + 1, sequence = sequence + 1
             WHERE id = ?3 AND state = 'verifying' AND version = ?4",
            params![resolved_name, now(), id, current_version],
        )?;
        if output_changed != 1 || state_changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn set_output_published(
        &mut self,
        id: &str,
        resolved_name: &str,
        final_path: &str,
        size_bytes: u64,
        sha256: &str,
        expected_partial_path: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let size_bytes = metadata_i64(size_bytes, "output size")?;
        let published_at = now();
        let changed = self.connection.execute(
            "UPDATE job_outputs
             SET resolved_name = ?1, final_path = ?2, size_bytes = ?3, sha256 = ?4,
                 status = 'published', published_at = ?5, partial_path = NULL
             WHERE job_id = ?6 AND ordinal = 0
               AND ((?7 IS NULL AND partial_path IS NULL) OR partial_path = ?7)",
            params![
                resolved_name,
                final_path,
                size_bytes,
                sha256,
                published_at,
                id,
                expected_partial_path,
            ],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn publication_evidence(
        &self,
        id: &str,
    ) -> Result<Option<PublicationEvidence>, DatabaseError> {
        self.connection
            .query_row(
                "SELECT final_path, resolved_name, size_bytes, sha256, status, partial_path
                 FROM job_outputs WHERE job_id = ?1 AND ordinal = 0
                   AND final_path IS NOT NULL AND resolved_name IS NOT NULL
                   AND size_bytes IS NOT NULL AND sha256 IS NOT NULL",
                [id],
                |row| {
                    let status: String = row.get(4)?;
                    let status = OutputStatus::from_contract(&status).ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            4,
                            "status".to_owned(),
                            rusqlite::types::Type::Text,
                        )
                    })?;
                    Ok(PublicationEvidence {
                        final_path: row.get(0)?,
                        resolved_name: row.get(1)?,
                        size_bytes: row_u64(row, 2)?,
                        sha256: row.get(3)?,
                        status,
                        partial_path: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_error(&mut self, id: &str, error: &OperationError) -> Result<(), DatabaseError> {
        if error.detail.len() > 500 || error.detail.chars().any(char::is_control) {
            return Err(DatabaseError::InvalidContractValue {
                field: "sanitized error detail",
            });
        }
        self.connection.execute(
            "INSERT INTO job_errors (
                job_id, code, title, sanitized_detail, stage, retryable,
                input_index, help_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                error.code,
                error.title,
                error.detail,
                error.stage.as_str(),
                error.retryable,
                error.input_index.map(|value| value as i64),
                error.help_id,
                now(),
            ],
        )?;
        Ok(())
    }

    pub fn record_error_once(
        &mut self,
        id: &str,
        error: &OperationError,
    ) -> Result<(), DatabaseError> {
        let already_recorded: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM job_errors WHERE job_id = ?1 AND code = ?2)",
            params![id, error.code],
            |row| row.get(0),
        )?;
        if !already_recorded {
            self.record_error(id, error)?;
        }
        Ok(())
    }

    pub fn in_flight_jobs(&self) -> Result<Vec<JobRecord>, DatabaseError> {
        let ids = {
            let mut statement = self.connection.prepare(
                "SELECT id FROM jobs WHERE state IN (
                    'queued', 'inspecting', 'preflight', 'ready', 'running', 'verifying',
                    'publishing', 'interrupted'
                 ) ORDER BY created_at",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        ids.iter()
            .map(|id| {
                self.get_job(id)?
                    .ok_or(DatabaseError::InvalidContractValue {
                        field: "in-flight job reference",
                    })
            })
            .collect()
    }

    pub fn startup_recovery_jobs(&self) -> Result<Vec<JobRecord>, DatabaseError> {
        let ids = {
            let mut statement = self.connection.prepare(
                "SELECT DISTINCT jobs.id
                 FROM jobs
                 LEFT JOIN job_outputs ON job_outputs.job_id = jobs.id
                 WHERE jobs.state IN (
                    'queued', 'inspecting', 'preflight', 'ready', 'running', 'verifying',
                    'publishing', 'interrupted'
                 )
                 OR (jobs.operation_id = ?1 AND jobs.operation_version = ?2)
                 OR job_outputs.staging_path IS NOT NULL OR job_outputs.partial_path IS NOT NULL
                 ORDER BY jobs.created_at",
            )?;
            let ids = statement
                .query_map(
                    params![DIAGNOSTIC_COPY_OPERATION_ID, LEGACY_DIAGNOSTIC_COPY_VERSION],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        ids.iter()
            .map(|id| {
                self.get_job(id)?
                    .ok_or(DatabaseError::InvalidContractValue {
                        field: "startup recovery job reference",
                    })
            })
            .collect()
    }

    pub fn mark_interrupted(
        &mut self,
        id: &str,
        expected_state: JobState,
    ) -> Result<(), DatabaseError> {
        if expected_state == JobState::Interrupted {
            self.connection.execute(
                "UPDATE jobs
                 SET stage = 'recovery', recovery_count = recovery_count + 1,
                     updated_at = ?1, version = version + 1, sequence = sequence + 1
                 WHERE id = ?2 AND state = 'interrupted'",
                params![now(), id],
            )?;
            return Ok(());
        }
        if !can_transition(expected_state, JobState::Interrupted) {
            return Err(DatabaseError::IllegalTransition);
        }
        let changed = self.connection.execute(
            "UPDATE jobs
             SET state = 'interrupted', stage = 'recovery', recovery_count = recovery_count + 1,
                 updated_at = ?1, version = version + 1, sequence = sequence + 1
             WHERE id = ?2 AND state = ?3",
            params![now(), id, expected_state.as_str()],
        )?;
        if changed != 1 {
            return Err(DatabaseError::JobConflict);
        }
        Ok(())
    }

    pub fn record_recovery_result_once(
        &mut self,
        id: &str,
        result_code: &str,
    ) -> Result<(), DatabaseError> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM job_stage_runs
                WHERE job_id = ?1 AND stage = 'recovery' AND safe_result_code = ?2
             )",
            params![id, result_code],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(());
        }
        let sequence: i64 = self.connection.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM job_stage_runs WHERE job_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let recorded_at = now();
        self.connection.execute(
            "INSERT INTO job_stage_runs (
                job_id, sequence, stage, started_at, finished_at,
                completed_units, total_units, safe_result_code
             ) VALUES (?1, ?2, 'recovery', ?3, ?3, 0, 0, ?4)",
            params![id, sequence, recorded_at, result_code],
        )?;
        Ok(())
    }

    pub fn legacy_cleanup_is_proven(&self, id: &str) -> Result<bool, DatabaseError> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM job_stage_runs
                    WHERE job_id = ?1 AND stage = 'recovery' AND safe_result_code = ?2
                 )",
                params![id, LEGACY_CLEANUP_PROVEN],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn upsert_dependency(
        &mut self,
        dependency: &DependencyDiagnostic,
    ) -> Result<(), DatabaseError> {
        let capabilities = serde_json::to_string(&dependency.capabilities)?;
        self.connection.execute(
            "INSERT INTO dependencies (
                id, kind, status, version, capabilities_json, scanned_at, safe_error_code
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind, status = excluded.status, version = excluded.version,
                capabilities_json = excluded.capabilities_json, scanned_at = excluded.scanned_at,
                safe_error_code = excluded.safe_error_code",
            params![
                dependency.id,
                match dependency.kind {
                    crate::contracts::DependencyKind::BuiltIn => "built-in",
                    crate::contracts::DependencyKind::External => "external",
                    crate::contracts::DependencyKind::Deferred => "deferred",
                },
                match dependency.status {
                    crate::contracts::DependencyStatus::Available => "available",
                    crate::contracts::DependencyStatus::Missing => "missing",
                    crate::contracts::DependencyStatus::Unhealthy => "unhealthy",
                    crate::contracts::DependencyStatus::Deferred => "deferred",
                    crate::contracts::DependencyStatus::NotRequired => "not-required",
                },
                dependency.version,
                capabilities,
                dependency.checked_at,
                dependency.error_code,
            ],
        )?;
        Ok(())
    }

    pub fn delete_terminal_history(&mut self, ids: &[String]) -> Result<usize, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for id in ids.iter().take(200) {
            let ambiguous: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM jobs
                    WHERE id = ?1 AND operation_id = ?2 AND operation_version = ?3
                      AND (
                        EXISTS(SELECT 1 FROM job_errors WHERE job_id = jobs.id AND code = ?4)
                        OR NOT EXISTS(
                            SELECT 1 FROM job_stage_runs
                            WHERE job_id = jobs.id AND stage = 'recovery' AND safe_result_code = ?5
                        )
                      )
                 )",
                params![
                    id,
                    DIAGNOSTIC_COPY_OPERATION_ID,
                    LEGACY_DIAGNOSTIC_COPY_VERSION,
                    LEGACY_CLEANUP_UNPROVEN,
                    LEGACY_CLEANUP_PROVEN,
                ],
                |row| row.get(0),
            )?;
            if ambiguous {
                return Err(DatabaseError::LegacyCleanupUnproven);
            }
        }
        let mut deleted = 0;
        for id in ids.iter().take(200) {
            deleted += transaction.execute(
                "DELETE FROM jobs WHERE id = ?1 AND state IN ('completed', 'failed', 'cancelled')",
                [id],
            )?;
        }
        transaction.commit()?;
        Ok(deleted)
    }

    pub fn purge_terminal_before(&mut self, cutoff: &str) -> Result<usize, DatabaseError> {
        self.connection
            .execute(
                "DELETE FROM jobs
                 WHERE id IN (
                    SELECT jobs.id FROM jobs
                    WHERE jobs.state IN ('completed', 'failed', 'cancelled')
                      AND jobs.finished_at IS NOT NULL AND jobs.finished_at < ?1
                      AND NOT (
                        jobs.operation_id = ?2 AND jobs.operation_version = ?3
                        AND (
                            EXISTS(
                                SELECT 1 FROM job_errors
                                WHERE job_id = jobs.id AND code = ?4
                            )
                            OR NOT EXISTS(
                                SELECT 1 FROM job_stage_runs
                                WHERE job_id = jobs.id AND stage = 'recovery'
                                  AND safe_result_code = ?5
                            )
                        )
                      )
                    ORDER BY jobs.finished_at, jobs.id
                    LIMIT ?6
                 )",
                params![
                    cutoff,
                    DIAGNOSTIC_COPY_OPERATION_ID,
                    LEGACY_DIAGNOSTIC_COPY_VERSION,
                    LEGACY_CLEANUP_UNPROVEN,
                    LEGACY_CLEANUP_PROVEN,
                    MAX_HISTORY_PURGE as i64,
                ],
            )
            .map_err(Into::into)
    }

    pub fn ensure_application_retention_setting(&mut self) -> Result<SettingRecord, DatabaseError> {
        if let Some(setting) = self.get_setting(HISTORY_RETENTION_SCOPE, HISTORY_RETENTION_KEY)? {
            validate_setting(&setting.scope, &setting.key, &setting.value, true)?;
            return Ok(setting);
        }
        self.set_setting(
            HISTORY_RETENTION_SCOPE,
            HISTORY_RETENTION_KEY,
            Value::from(DEFAULT_HISTORY_RETENTION_DAYS),
            0,
        )
    }

    pub fn run_retention_at(
        &mut self,
        maintenance_time: DateTime<Utc>,
    ) -> Result<usize, DatabaseError> {
        let setting = self.ensure_application_retention_setting()?;
        let days = setting
            .value
            .as_u64()
            .filter(|days| *days <= 365)
            .ok_or(DatabaseError::SettingNotAllowed)?;
        let days = i64::try_from(days).map_err(|_| DatabaseError::SettingNotAllowed)?;
        let cutoff = maintenance_time - ChronoDuration::days(days);
        self.purge_terminal_before(&cutoff.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn get_setting(
        &self,
        scope: &str,
        key: &str,
    ) -> Result<Option<SettingRecord>, DatabaseError> {
        validate_setting(scope, key, &Value::Null, false)?;
        self.connection
            .query_row(
                "SELECT value_json, version, updated_at FROM settings WHERE scope = ?1 AND key = ?2",
                params![scope, key],
                |row| {
                    let value_json: String = row.get(0)?;
                    Ok((value_json, row_u64(row, 1)?, row.get::<_, String>(2)?))
                },
            )
            .optional()?
            .map(|(value_json, version, updated_at)| {
                Ok(SettingRecord {
                    scope: scope.to_owned(),
                    key: key.to_owned(),
                    value: serde_json::from_str(&value_json)?,
                    version,
                    updated_at,
                })
            })
            .transpose()
    }

    pub fn set_setting(
        &mut self,
        scope: &str,
        key: &str,
        value: Value,
        expected_version: u64,
    ) -> Result<SettingRecord, DatabaseError> {
        validate_setting(scope, key, &value, true)?;
        let value_json = serde_json::to_string(&value)?;
        if value_json.len() > 4096 {
            return Err(DatabaseError::SettingNotAllowed);
        }
        let updated_at = now();
        let next_version = expected_version + 1;
        let next_version_i64 = metadata_i64(next_version, "setting version")?;
        let expected_version_i64 = metadata_i64(expected_version, "setting version")?;
        let changed = if expected_version == 0 {
            self.connection.execute(
                "INSERT OR IGNORE INTO settings(scope, key, value_json, version, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![scope, key, value_json, next_version_i64, updated_at],
            )?
        } else {
            self.connection.execute(
                "UPDATE settings SET value_json = ?1, version = ?2, updated_at = ?3
                 WHERE scope = ?4 AND key = ?5 AND version = ?6",
                params![
                    value_json,
                    next_version_i64,
                    updated_at,
                    scope,
                    key,
                    expected_version_i64,
                ],
            )?
        };
        if changed != 1 {
            return Err(DatabaseError::SettingConflict);
        }
        Ok(SettingRecord {
            scope: scope.to_owned(),
            key: key.to_owned(),
            value,
            version: next_version,
            updated_at,
        })
    }

    fn terminal_evidence_is_ready(&self, id: &str, next: JobState) -> Result<bool, DatabaseError> {
        let temporary_paths: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM job_outputs
             WHERE job_id = ?1 AND (staging_path IS NOT NULL OR partial_path IS NOT NULL)",
            [id],
            |row| row.get(0),
        )?;
        if temporary_paths != 0 {
            return Ok(false);
        }
        if next != JobState::Completed {
            return Ok(true);
        }
        let published: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM job_outputs
             WHERE job_id = ?1 AND status = 'published' AND final_path IS NOT NULL
               AND size_bytes IS NOT NULL AND sha256 IS NOT NULL AND published_at IS NOT NULL",
            [id],
            |row| row.get(0),
        )?;
        Ok(published > 0)
    }

    fn load_inputs(&self, id: &str) -> Result<Vec<JobInput>, DatabaseError> {
        let mut statement = self.connection.prepare(
            "SELECT ordinal, display_name, source_path, canonical_path, file_identity,
                    size_bytes, modified_at, mime_type, sha256, password_reference
             FROM job_inputs WHERE job_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok(JobInput {
                ordinal: row.get(0)?,
                display_name: row.get(1)?,
                source_path: row.get(2)?,
                canonical_path: row.get(3)?,
                file_identity: row.get(4)?,
                size_bytes: row_u64(row, 5)?,
                modified_at: row.get(6)?,
                mime_type: row.get(7)?,
                sha256: row.get(8)?,
                password_reference: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn load_outputs(&self, id: &str) -> Result<Vec<JobOutput>, DatabaseError> {
        let mut statement = self.connection.prepare(
            "SELECT ordinal, requested_name, resolved_name, staging_path, partial_path,
                    final_path, size_bytes, mime_type, sha256, status, verified_at, published_at
             FROM job_outputs WHERE job_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([id], |row| {
            let status: String = row.get(9)?;
            let status = OutputStatus::from_contract(&status).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    9,
                    "status".to_owned(),
                    rusqlite::types::Type::Text,
                )
            })?;
            Ok(JobOutput {
                ordinal: row.get(0)?,
                requested_name: row.get(1)?,
                resolved_name: row.get(2)?,
                staging_path: row.get(3)?,
                partial_path: row.get(4)?,
                final_path: row.get(5)?,
                size_bytes: row_optional_u64(row, 6)?,
                mime_type: row.get(7)?,
                sha256: row.get(8)?,
                status,
                verified_at: row.get(10)?,
                published_at: row.get(11)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn load_errors(&self, id: &str) -> Result<Vec<OperationError>, DatabaseError> {
        let mut statement = self.connection.prepare(
            "SELECT code, title, sanitized_detail, stage, retryable, input_index, help_id
             FROM job_errors WHERE job_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([id], |row| {
            let stage: String = row.get(3)?;
            let stage = OperationStage::from_contract(&stage).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    3,
                    "stage".to_owned(),
                    rusqlite::types::Type::Text,
                )
            })?;
            Ok(OperationError {
                code: row.get(0)?,
                title: row.get(1)?,
                detail: row.get(2)?,
                stage,
                retryable: row.get(4)?,
                input_index: row_optional_usize(row, 5)?,
                help_id: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub fn configure_connection(connection: &Connection) -> Result<(), DatabaseError> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

pub fn apply_migrations(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), DatabaseError> {
    connection.execute_batch(MIGRATION_LEDGER_SQL)?;
    let expected_versions: HashSet<i64> = migrations
        .iter()
        .map(|migration| migration.version)
        .collect();
    {
        let mut statement = connection.prepare("SELECT version FROM schema_migrations")?;
        let applied = statement.query_map([], |row| row.get::<_, i64>(0))?;
        for version in applied {
            let version = version?;
            if !expected_versions.contains(&version) {
                return Err(DatabaseError::UnknownMigration { version });
            }
        }
    }

    for migration in migrations {
        let checksum = sha256_hex(migration.sql.as_bytes());
        let recorded: Option<String> = connection
            .query_row(
                "SELECT sql_checksum FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(recorded) = recorded {
            if recorded != checksum {
                return Err(DatabaseError::MigrationChecksum {
                    version: migration.version,
                });
            }
            continue;
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, sql_checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![migration.version, migration.name, checksum, now()],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn metadata_i64(value: u64, field: &'static str) -> Result<i64, DatabaseError> {
    i64::try_from(value).map_err(|_| DatabaseError::InvalidContractValue { field })
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn row_optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn row_optional_usize(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<usize>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn validate_setting(
    scope: &str,
    key: &str,
    value: &Value,
    validate_value: bool,
) -> Result<(), DatabaseError> {
    if !matches!(scope, "application" | "operation") {
        return Err(DatabaseError::SettingNotAllowed);
    }
    let allowed = match key {
        HISTORY_RETENTION_KEY => scope == HISTORY_RETENTION_SCOPE,
        "privacy.offline" | "ui.theme" => true,
        _ => false,
    };
    if !allowed {
        return Err(DatabaseError::SettingNotAllowed);
    }
    if validate_value {
        let valid = match key {
            "history.retention_days" => value.as_u64().is_some_and(|days| days <= 365),
            "privacy.offline" => value.is_boolean(),
            "ui.theme" => value
                .as_str()
                .is_some_and(|theme| matches!(theme, "system" | "light" | "dark")),
            _ => false,
        };
        if !valid {
            return Err(DatabaseError::SettingNotAllowed);
        }
    }
    Ok(())
}
