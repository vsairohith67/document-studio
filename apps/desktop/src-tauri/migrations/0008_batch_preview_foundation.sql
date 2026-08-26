CREATE TABLE batch_runs (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    operation_id TEXT NOT NULL CHECK (operation_id = 'pdf.compress-lossless'),
    operation_version TEXT NOT NULL CHECK (operation_version = '1.0.0'),
    state TEXT NOT NULL CHECK (
      state IN ('queued', 'active', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    preview_sha256 TEXT NOT NULL
      CHECK (length(preview_sha256) = 64 AND preview_sha256 NOT GLOB '*[^0-9a-f]*'),
    plan_key_sha256 TEXT NOT NULL
      CHECK (length(plan_key_sha256) = 64 AND plan_key_sha256 NOT GLOB '*[^0-9a-f]*'),
    settings_sha256 TEXT NOT NULL
      CHECK (length(settings_sha256) = 64 AND settings_sha256 NOT GLOB '*[^0-9a-f]*'),
    naming_template TEXT NOT NULL CHECK (length(CAST(naming_template AS BLOB)) BETWEEN 6 AND 1024),
    optimistic_version INTEGER NOT NULL CHECK (optimistic_version >= 0),
    destination_directory TEXT NOT NULL CHECK (length(destination_directory) BETWEEN 1 AND 32767),
    destination_identity TEXT NOT NULL CHECK (length(destination_identity) BETWEEN 1 AND 128),
    workspace_peak_bytes INTEGER NOT NULL CHECK (workspace_peak_bytes >= 0),
    destination_total_bytes INTEGER NOT NULL CHECK (destination_total_bytes >= 0),
    combined_required_bytes INTEGER NOT NULL CHECK (
      combined_required_bytes >= workspace_peak_bytes
      AND combined_required_bytes >= destination_total_bytes
    ),
    shared_volume INTEGER NOT NULL CHECK (shared_volume IN (0, 1)),
    total_children INTEGER NOT NULL CHECK (total_children BETWEEN 1 AND 128),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    UNIQUE (plan_key_sha256, optimistic_version)
) STRICT;

CREATE UNIQUE INDEX batch_runs_one_live_plan_idx
  ON batch_runs(plan_key_sha256)
  WHERE state IN ('queued', 'active');

CREATE TABLE batch_run_jobs (
    batch_id TEXT NOT NULL REFERENCES batch_runs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 127),
    job_id TEXT NOT NULL UNIQUE REFERENCES jobs(id) ON DELETE RESTRICT,
    requested_name TEXT NOT NULL CHECK (length(requested_name) BETWEEN 1 AND 255),
    planned_name TEXT NOT NULL CHECK (length(planned_name) BETWEEN 1 AND 255),
    collision_index INTEGER NOT NULL CHECK (collision_index BETWEEN 0 AND 999),
    PRIMARY KEY (batch_id, ordinal)
) STRICT;

CREATE INDEX batch_run_jobs_job_id_idx ON batch_run_jobs(job_id);
