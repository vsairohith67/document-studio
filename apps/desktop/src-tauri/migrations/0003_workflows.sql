CREATE TABLE workflows (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
CREATE TABLE workflow_steps (
    workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    operation_id TEXT NOT NULL,
    operation_version TEXT NOT NULL,
    settings_json TEXT NOT NULL CHECK (json_valid(settings_json) AND length(settings_json) <= 65536),
    PRIMARY KEY (workflow_id, ordinal)
) STRICT;

CREATE TABLE workflow_runs (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE RESTRICT,
    workflow_version INTEGER NOT NULL CHECK (workflow_version > 0),
    status TEXT NOT NULL CHECK (status IN ('planned', 'running', 'completed', 'failed', 'cancelled', 'interrupted')),
    variables_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(variables_json) AND length(variables_json) <= 65536),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT
) STRICT;

CREATE INDEX workflow_runs_status_idx ON workflow_runs(status, updated_at);

CREATE TABLE workflow_run_jobs (
    workflow_run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    step_ordinal INTEGER NOT NULL CHECK (step_ordinal >= 0),
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE RESTRICT,
    PRIMARY KEY (workflow_run_id, step_ordinal),
    UNIQUE (job_id)
) STRICT;
