# API and IPC Design

## Desktop IPC

The React app may call only named Tauri commands with serializable typed payloads. It cannot execute arbitrary binaries or write unrestricted paths.

### Command groups

- `system_status`
- `operations_list`
- `files_inspect`
- `jobs_create`
- `jobs_cancel`
- `jobs_resolve_interrupted`
- `jobs_get`
- `history_list`
- `history_delete`
- `dependencies_scan`
- `settings_get`
- `settings_set`

These are the exact G01 Tauri command names. The frontend groups them behind typed methods such as `api.jobs.create`. Rust validates every request again. Unknown operations, settings, paths and oversized lists fail with a structured `OperationError`; raw database, OS and debug errors do not cross IPC.

G01 emits one advisory event, `document-studio-job-progress-v1`. It contains schema version 1, a monotonic per-job sequence, UTC time, job and operation IDs, state, stage, completed/total units, unit, safe message code/text and whether cancellation is currently available. It contains no full path, document content or secret. Events are throttled during byte copying and always emitted for state/stage changes.

SQLite-backed `jobs_get` and `history_list` are authoritative. The client ignores duplicate/stale sequences and calls `jobs_get` after a sequence gap or reload.

`jobs_resolve_interrupted` performs one evidence-based, non-resuming reconciliation. It completes only already-published output with matching durable evidence, otherwise fails after exact cleanup, and remains interrupted on cleanup failure. It rejects ambiguous `1.0.0` records with `LEGACY_CLEANUP_UNPROVEN`. `history_delete` exposes the same safe error instead of deleting a mixed request partially. Settings IPC permits retention only at `application/history.retention_days`.

## Future cloud API

- REST/JSON for job submission, metadata and downloads.
- Server-sent events or WebSocket for progress.
- Idempotency key for submissions.
- Pre-signed upload/download URLs with short expiry.
- Explicit operation/version and schema version.
- Authentication, rate limits and regional policy.

## Error envelope

```json
{
  "code": "INPUT_ENCRYPTED",
  "title": "This PDF needs a password",
  "detail": "Provide the opening password to continue.",
  "inputIndex": 1,
  "stage": "preflight",
  "retryable": true,
  "helpId": "pdf-passwords"
}
```

## G02 IPC behavior

`jobs_create` is a discriminated request: `diagnostic.copy` requires one input and `pdf.merge` requires 2–128. `files_inspect` accepts up to 128 paths and reports `application/pdf` only when extension and header inspection agree. Rust validates every path and ordinal again.

The frontend submits its displayed array unchanged. It freezes the list after creation, follows `document-studio-job-progress-v1`, and reconciles from `jobs_get` after reload or an event gap. Native drop paths pass through the same inspection command as the file chooser. No general filesystem, shell, HTTP, or expanded Tauri capability is exposed.
