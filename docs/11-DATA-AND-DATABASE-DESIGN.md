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

## Retention

- Job history is configurable.
- Temporary workspaces are removed after terminal states and reconciled at startup.
- Extracted text/embeddings are opt-in and scoped to AI features.
- Passwords/API keys/certificate secrets are stored in the OS credential store, referenced by opaque IDs.

## Future cloud storage

PostgreSQL stores account, job and policy metadata. Object storage holds encrypted inputs/outputs with short TTLs. Redis/Valkey coordinates queues and locks. Storage region and deletion deadline are recorded per object.
