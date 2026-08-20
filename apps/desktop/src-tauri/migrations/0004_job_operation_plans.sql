CREATE TABLE job_operation_plans (
    job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    operation_id TEXT NOT NULL CHECK (length(operation_id) BETWEEN 3 AND 64),
    source_page_count INTEGER NOT NULL CHECK (source_page_count BETWEEN 1 AND 4096),
    plan_json TEXT NOT NULL CHECK (
        json_valid(plan_json)
        AND length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536
    ),
    plan_sha256 TEXT NOT NULL CHECK (
        length(plan_sha256) = 64
        AND plan_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX job_operation_plans_operation_idx
    ON job_operation_plans(operation_id, created_at DESC);
