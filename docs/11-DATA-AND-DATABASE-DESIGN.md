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

Job creation, compare-and-set state transitions, publication evidence, audit updates and retention deletion use explicit transactions. Filesystem publication is reconciled with recorded evidence because SQLite and the filesystem cannot share one transaction.

## Retention

- Terminal job history defaults to 30 days and can be deleted immediately.
- Temporary workspaces are removed after terminal states and reconciled at startup.
- Interrupted records remain until recovery, retry or explicit history deletion.
- G01 stores no document bodies, extracted text, page images, thumbnails, embeddings, passwords, keys, prompts, clipboard data, arbitrary logs or binary payloads.
- A future secret may be referenced by an opaque credential-store ID, but G01 stores no document password.

## Future cloud storage

PostgreSQL stores account, job and policy metadata. Object storage holds encrypted inputs/outputs with short TTLs. Redis/Valkey coordinates queues and locks. Storage region and deletion deadline are recorded per object.
