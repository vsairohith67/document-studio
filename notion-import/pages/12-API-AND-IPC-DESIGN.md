# API and IPC Design

## Desktop IPC

The React app may call only named Tauri commands with serializable typed payloads. It cannot execute arbitrary binaries or write unrestricted paths.

### Command groups

- `files.inspect`
- `operations.list`
- `jobs.create`
- `jobs.cancel`
- `jobs.get`
- `jobs.subscribe`
- `history.list`
- `history.delete`
- `dependencies.scan`
- `settings.get/set`
- `models.list/install/remove`

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
