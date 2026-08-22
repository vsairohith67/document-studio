CREATE TABLE job_operation_specs (
    job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    operation_id TEXT NOT NULL CHECK (length(operation_id) BETWEEN 3 AND 64),
    settings_json TEXT NOT NULL CHECK (
        json_valid(settings_json)
        AND length(CAST(settings_json AS BLOB)) BETWEEN 2 AND 65536
    ),
    settings_sha256 TEXT NOT NULL CHECK (
        length(settings_sha256) = 64
        AND settings_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX job_operation_specs_operation_idx
    ON job_operation_specs(operation_id, created_at DESC);

CREATE TABLE job_warnings (
    id INTEGER PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (length(code) BETWEEN 3 AND 64),
    sanitized_detail TEXT NOT NULL CHECK (length(sanitized_detail) BETWEEN 1 AND 500),
    input_index INTEGER CHECK (input_index IS NULL OR input_index BETWEEN 0 AND 127),
    page_index INTEGER CHECK (page_index IS NULL OR page_index BETWEEN 0 AND 127),
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX job_warnings_job_created_idx ON job_warnings(job_id, created_at, id);
