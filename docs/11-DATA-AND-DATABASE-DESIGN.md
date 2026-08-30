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
- `job_completion_outcomes`: optional strict completion-kind metadata added by migration 6; one row per job at most, with cascade deletion.
- `batch_runs`, `batch_run_jobs`: migration 8 metadata for private preview proofs, CAS/live-plan ownership and stable links to ordinary queued child jobs; no scheduler.

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

## G04C2A migration 6 and successful no-publication outcomes

`0006_job_completion_outcomes.sql` adds one optional strict row keyed to `jobs.id`. `completion_kind` is only `published` or `no-benefit`; `reason` is null for published and exactly `savings-threshold-not-met` for no-benefit. The null-safe SQLite constraint rejects missing and arbitrary no-benefit reasons. The table stores only the job ID, enum values and creation time—no path, generic JSON, document content, candidate bytes or backfill.

## G04C2B migration 7

`0007_balanced_compression_audits.sql` adds one optional strict scalar audit per balanced job plus closed, positive skip counts. It stores source/candidate sizes, selected/skipped counts, affected/compared pages, minimum metrics, maximum changed-pixel evidence, quality/size gate booleans, one structural-proof SHA-256 and a timestamp. Foreign keys cascade with the owning job. The repository writes the audit atomically while the job is verifying and rejects duplicate, malformed, mismatched aggregate or non-balanced evidence. No qpdf JSON, image/PDF bytes, raw pixels, document content, arbitrary JSON or new path is stored, and migration 7 performs no backfill.

Loading an explicit outcome validates the full cross-table contract. Published requires completed state, a finish time, a resolved output name, at least one fully published output, accepted final evidence and no staging/partial ownership. No-benefit requires completed state, a finish time, zero outputs, zero errors, no resolved output name and no temporary output ownership. Existing rows without an outcome load as `null`/`null` and keep their prior evidence rules.

No-benefit completion is one `IMMEDIATE` transaction: exact state/version/cancellation/output/error/outcome/name preconditions, exact outcome insertion, and the terminal state/progress/time/sequence update. A failed insert or update rolls back both. The internal published helper likewise records the published outcome in the same transaction as the future operation's truthful terminal update. Neither helper is exposed through IPC.

Retention and `history_delete` validate explicit outcomes before deletion; a valid no-benefit row cascades with its job without a filesystem operation. Startup recovery includes every job with an explicit outcome so invalid combinations fail closed. Valid completed no-benefit metadata is left untouched and has no final path to reconcile.

## G04F1 migration 8 and atomic batch metadata

`0008_batch_preview_foundation.sql` follows accepted G04C2B migration 7 without changing migrations 1–7. `batch_runs` stores only closed proof metadata: schema/operation/version, preview and plan-key hashes, exact `{}` settings hash, naming template, optimistic version, trusted local destination identity/path, checked estimates, child count, state and timestamps. A partial unique index permits only one queued/active batch per plan key. `batch_run_jobs` stores stable ordinals and naming decisions. Neither table stores canonical preview JSON or document content.

After full recomputation, one `IMMEDIATE` transaction checks the optimistic version/live-plan gate and inserts the batch, ordinary queued child jobs, inputs, planned outputs, canonical operation specs and ordered links. Any injected insert failure rolls back every row. Unstarted queued children are excluded from worker-start and startup-recovery selection, while history deletion does not orphan linked metadata. Existing ordinary published/null legacy and explicit published/no-benefit outcomes load without reinterpretation.

## G04E1 metadata reuse and no migration

G04E1 requires no migration. Existing jobs, inputs, outputs, canonical operation specs, stage runs, warnings and dependency diagnostics contain every durable scalar needed for `text.to-pdf@1.0.0`: opaque job/session binding, private source identity/size/modification/hash, closed settings, progress/state/version, staging/publication size/hash and sanitized runtime/dependency status.

TXT bytes, normalized text, HTML, CSS responses, WebView2 UDF data, raw/normalized private PDFs and the private audit file remain generation-owned workspace data and are deleted after verified publication/recovery reconciliation. SQLite never stores source text, extracted text, a React preview, COM response bytes or WebView profile contents. The exact WebView2 runtime version is an allow-listed scalar in the existing dependency table; no job-schema or migration 9 field is invented.

The existing `publishing` output row is also the durable commit-ambiguity record. If the no-overwrite move succeeds but final reopen or metadata finalization fails, the service does not clear its final path, expected size/SHA-256 or owned-partial path. Startup may promote that row only when the owned partial is absent and the exact final path reopens as a regular non-reparse file with the expected hash; mismatch or access ambiguity remains interrupted and retryable while preserving the user file. This uses existing fields and adds no migration.
