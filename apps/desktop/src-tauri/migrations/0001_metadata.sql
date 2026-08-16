CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    name TEXT NOT NULL UNIQUE,
    sql_checksum TEXT NOT NULL CHECK (length(sql_checksum) = 64),
    applied_at TEXT NOT NULL
) STRICT;
CREATE TABLE settings (
    scope TEXT NOT NULL CHECK (scope IN ('application', 'operation')),
    key TEXT NOT NULL,
    value_json TEXT NOT NULL CHECK (json_valid(value_json) AND length(value_json) <= 4096),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (scope, key)
) STRICT;

CREATE TABLE dependencies (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('built-in', 'external', 'deferred')),
    status TEXT NOT NULL CHECK (status IN ('available', 'missing', 'unhealthy', 'deferred', 'not-required')),
    path TEXT,
    version TEXT,
    health TEXT,
    capabilities_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(capabilities_json)),
    scanned_at TEXT NOT NULL,
    safe_error_code TEXT
) STRICT;

CREATE INDEX dependencies_status_idx ON dependencies(status, scanned_at DESC);

CREATE TABLE presets (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    operation_version TEXT NOT NULL,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL CHECK (json_valid(settings_json) AND length(settings_json) <= 65536),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (operation_id, name)
) STRICT;
