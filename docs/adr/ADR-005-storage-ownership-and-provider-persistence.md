# ADR-005: Storage ownership and provider persistence

Status: Accepted

## Decision

Document Studio is not a permanent document store. G01 accepts only user-selected local input files and destination directories. Inputs remain user-owned filesystem references and are streamed when an operation runs. The application must not silently copy source documents into permanent application-controlled storage.

Each active job may use a private, UUID-scoped workspace below the application data directory. That workspace and any destination-local partial file are temporary implementation details, not user storage. They are recorded only so the application can perform exact cleanup and startup recovery. Final documents are published only to a user-selected local destination.

SQLite stores metadata only. It may store allow-listed paths, sizes, timestamps, safe SHA-256 fingerprints, operation and lifecycle state, dependency health, settings, sanitized errors, and cleanup/publication evidence. It must not store document bodies, page images, extracted text, thumbnails, passwords, keys, prompts, clipboard content, embeddings, or arbitrary binary/log payloads.

Terminal job metadata is retained for 30 days by default and can normally be deleted immediately by the user. ADR-007 quarantines pre-fix legacy records whose destination cleanup is unproven. Temporary document data is removed after success, failure, cancellation, or recovery. A cleanup failure keeps the job recoverable instead of falsely claiming a clean terminal state.

G01 implements no accounts, Google Drive, OneDrive, SharePoint, cloud worker, hosted processing, or external AI persistence. A future provider requires a separate ADR covering explicit authorization, the provider-owned persistence model, metadata allow-list, retention deadline, deletion evidence, consent revocation, offline behavior, and error recovery.

## Rationale

The ownership boundary preserves the local-first privacy promise while still allowing durable metadata and safe crash recovery. A bounded metadata history is useful for support and retry without turning the application data directory into an undisclosed document archive.

## Consequences

- Published user documents are never removed by history deletion, cleanup, recovery, or rollback.
- Workspace cleanup is limited to validated UUID directories with application ownership markers.
- Destination-local partial files use random, recorded names and are never presented as completed outputs.
- Provider persistence remains blocked until a later goal and ADR explicitly approve it.
