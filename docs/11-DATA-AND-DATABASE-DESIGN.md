# Data and Database Design

## Desktop storage

SQLite stores metadata only. Documents remain in user-selected locations or short-lived job workspaces.

### Core entities

- `jobs`: state, operation, timestamps, progress, stage, destination, verification and error.
- `job_inputs`: path, fingerprint, MIME, size, page count and password reference.
- `job_outputs`: staging/final path, MIME, size, page count and verification result.
- `presets`: operation-scoped settings JSON.
- `workflows`: versioned directed sequence of operation steps.
- `workflow_runs`: resolved variables and per-step job IDs.
- `dependencies`: path, version, health and capabilities.
- `models`: model ID, revision, license, files, verification and install state.
- `settings`: scoped key/value store with migration version.
- `signature_identities`: references to protected local assets/certificates; no plaintext secrets.

### G01 implemented schema

The G01 database is opened by one blocking Rust repository worker. It enables foreign keys, WAL journaling, `synchronous=FULL`, and a bounded busy timeout. Ordered migrations are checksummed and applied in transactions. An unknown or changed migration fails startup closed.

- `schema_migrations`: applied version, name, checksum and UTC time.
- `settings`: allow-listed scope/key JSON with optimistic versioning.
- `dependencies`: built-in or deferred dependency health and safe diagnostics.
- `presets`: operation-scoped metadata and settings only.
- `jobs`, `job_inputs`, `job_outputs`, `job_stage_runs`, `job_errors`: durable lifecycle, progress, paths, identities, hashes, publication evidence and sanitized errors.
- `workflows`, `workflow_steps`, `workflow_runs`, `workflow_run_jobs`: metadata foundation only; G01 does not execute workflows.

Constraints restrict states, stages, statuses, non-negative units and ordinals, JSON validity and SHA-256 length. Indexed fields include state/update time, operation/create time, retention time, dependency status and workflow-run status. No migration contains a BLOB column.

Job creation, compare-and-set state transitions, exact publication-partial reservation/release, publication evidence, audit updates and retention deletion use explicit transactions. A `partial_path` row is only a reservation. After guarded creation, `job_stage_runs.safe_result_code` stores a path-UUID-and-Windows-file-identity activation token; deletion requires both the current exact path and the matching opened file identity. Filesystem publication is reconciled with recorded evidence because SQLite and the filesystem cannot share one transaction. No migration was added for the acceptance remediation: schema-v3 already has `jobs.operation_version`, `job_outputs.partial_path`, `job_errors`, and `job_stage_runs.safe_result_code`.

## Retention

- Retention is application-only: `application/history.retention_days` is accepted and `operation/history.retention_days` is rejected. The integer range is `0..=365`, with 30 initialized when missing.
- Startup runs recovery first, then deletes at most 1,000 oldest eligible terminal records older than one injected UTC cutoff. A successful retention-setting update runs the same bounded maintenance path.
- Temporary workspaces are removed after terminal states and reconciled at startup.
- Active/interrupted records are never purged. Ambiguous legacy `diagnostic.copy` `1.0.0` records carry `LEGACY_CLEANUP_UNPROVEN` and are rejected by both automatic retention and all-or-nothing `history_delete` preflight. Only pre-publication legacy work with durable `LEGACY_CLEANUP_PROVEN` can re-enter ordinary retention.
- G01 stores no document bodies, extracted text, page images, thumbnails, embeddings, passwords, keys, prompts, clipboard data, arbitrary logs or binary payloads.
- A future secret may be referenced by an opaque credential-store ID, but G01 stores no document password.

## Future cloud storage

PostgreSQL stores account, job and policy metadata. Object storage holds encrypted inputs/outputs with short TTLs. Redis/Valkey coordinates queues and locks. Storage region and deletion deadline are recorded per object.

## G02 storage use

G02 adds no migration and no BLOB column. Existing `job_inputs.ordinal` stores exact merge order; each row stores only the already-authorized path/identity/size/time/MIME/hash metadata required for durable execution and recovery. Duplicate or hard-linked inputs retain separate ordinals and separate physical workspace snapshots.

Source PDFs remain in their original locations and are never rewritten. Temporary PDF bytes exist only in the marker-owned per-job workspace until verified publication. SQLite stores staging/final hash and size evidence, dependency version, safe lifecycle codes, and sanitized errors—never PDF bytes, page text, raw qpdf output, passwords, thumbnails, or document metadata.

## G03 migration 4 and multi-output records

`0004_job_operation_plans.sql` adds one `job_operation_plans` row per G03 output job:

- `job_id` primary/foreign key with cascade delete;
- `schema_version = 1`, exact `operation_id`, and positive `source_page_count`;
- canonical `plan_json` with `json_valid(plan_json)` and `length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536`;
- 64-character lowercase SHA-256 and UTC creation time.

Rust independently validates the exact UTF-8 byte length, operation/payload match, page bounds, output names, index uniqueness/permutation/ranges and canonical hash before insertion and again before execution. Migration 4 adds no BLOB column and no document body, thumbnail, extracted text, search index or password.

All expected `job_outputs` rows, with stable ordinals and requested names, are inserted in the same job-creation transaction before processing. Completion is legal only when every expected output is `published` with final path/hash/size evidence. A later publication failure leaves earlier published rows intact, records `PARTIAL_PUBLICATION`, and terminates the job as failed. Recovery preserves published user files and never infers all-or-nothing publication.

Older `diagnostic.copy` and `pdf.merge` jobs have no plan row and remain valid. Migration checksums are append-only; an older binary must not open schema 4. Rollback uses a pre-migration backup rather than an in-place table drop.
