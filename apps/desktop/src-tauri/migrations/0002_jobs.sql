CREATE TABLE jobs (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    operation_id TEXT NOT NULL,
    operation_version TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'queued', 'inspecting', 'preflight', 'ready', 'running', 'verifying',
        'publishing', 'completed', 'failed', 'cancelled', 'interrupted'
    )),
    stage TEXT CHECK (stage IS NULL OR stage IN (
        'inspect', 'preflight', 'estimate', 'plan', 'execute', 'verify',
        'publish', 'audit', 'cleanup', 'recovery'
    )),
    sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    completed_units INTEGER NOT NULL DEFAULT 0 CHECK (completed_units >= 0),
    total_units INTEGER NOT NULL DEFAULT 0 CHECK (total_units >= 0),
    unit TEXT NOT NULL DEFAULT 'steps' CHECK (unit IN ('bytes', 'items', 'steps')),
    destination_directory TEXT NOT NULL,
    requested_output_name TEXT NOT NULL,
    resolved_output_name TEXT,
    cancellation_requested_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    recovery_count INTEGER NOT NULL DEFAULT 0 CHECK (recovery_count >= 0),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    CHECK (completed_units <= total_units OR total_units = 0)
) STRICT;

CREATE INDEX jobs_state_updated_idx ON jobs(state, updated_at);
CREATE INDEX jobs_operation_created_idx ON jobs(operation_id, created_at DESC);
CREATE INDEX jobs_terminal_retention_idx ON jobs(finished_at) WHERE state IN ('completed', 'failed', 'cancelled');

CREATE TABLE job_inputs (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    display_name TEXT NOT NULL,
    source_path TEXT NOT NULL,
    canonical_path TEXT NOT NULL,
    file_identity TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_at TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    sha256 TEXT CHECK (sha256 IS NULL OR (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*')),
    password_reference TEXT,
    PRIMARY KEY (job_id, ordinal)
) STRICT;

CREATE TABLE job_outputs (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    requested_name TEXT NOT NULL,
    resolved_name TEXT,
    staging_path TEXT,
    partial_path TEXT,
    final_path TEXT,
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    mime_type TEXT NOT NULL,
    sha256 TEXT CHECK (sha256 IS NULL OR (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*')),
    status TEXT NOT NULL CHECK (status IN ('planned', 'staged', 'verified', 'publishing', 'published')),
    verified_at TEXT,
    published_at TEXT,
    PRIMARY KEY (job_id, ordinal)
) STRICT;

CREATE TABLE job_stage_runs (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    stage TEXT NOT NULL CHECK (stage IN (
        'inspect', 'preflight', 'estimate', 'plan', 'execute', 'verify',
        'publish', 'audit', 'cleanup', 'recovery'
    )),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    completed_units INTEGER NOT NULL DEFAULT 0 CHECK (completed_units >= 0),
    total_units INTEGER NOT NULL DEFAULT 0 CHECK (total_units >= 0),
    safe_result_code TEXT,
    PRIMARY KEY (job_id, sequence)
) STRICT;

CREATE TABLE job_errors (
    id INTEGER PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    sanitized_detail TEXT NOT NULL CHECK (length(sanitized_detail) <= 500),
    stage TEXT NOT NULL CHECK (stage IN (
        'inspect', 'preflight', 'estimate', 'plan', 'execute', 'verify',
        'publish', 'audit', 'cleanup', 'recovery'
    )),
    retryable INTEGER NOT NULL CHECK (retryable IN (0, 1)),
    input_index INTEGER CHECK (input_index IS NULL OR input_index >= 0),
    help_id TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX job_errors_job_created_idx ON job_errors(job_id, created_at);
