# ADR-008: Windows single-instance cancellation ownership

Status: Accepted

## Decision

G01 registers official crate `tauri-plugin-single-instance` `~2.4.3` before every other Tauri plugin. On Windows, the application identifier derives the process-wide single-instance mutex/message endpoint. A secondary process exits through the plugin before Tauri setup can open SQLite, initialize workspaces, register job tokens or run workers.

The primary callback ignores forwarded arguments and working directory. It only restores, shows and focuses the existing `main` window. No forwarded input can create work or alter paths. The Rust-only plugin adds no webview command and requires no Tauri capability permission.

Cancellation tokens remain process-local. IPC registers the token synchronously before worker spawn. Spawn failure unregisters it and deterministically fails the queued job after cleanup. Cancellation and publication commit use one atomic winner; after commit begins, cancellation returns `CANCEL_TOO_LATE`. If no token exists, cancellation reaches a terminal state only after exact partial/workspace cleanup evidence. Ambiguous legacy work returns `LEGACY_CLEANUP_UNPROVEN`.

## Rejected alternative

Durable cross-process ownership leases, cancellation flags and worker polling were rejected for G01. They would require new schema fields, lease expiry, stale-owner election and race semantics even though G01 has one local reference worker and does not implement rescheduling/resume.

## Consequences

- One official MIT OR Apache-2.0 Rust dependency and its locked transitive graph are added.
- A Windows two-process smoke uses a test-only app-data override/marker to prove the secondary never reaches setup. Production builds exclude `test-runtime`.
- Future multi-process workers must supersede this ADR with durable ownership/lease design before sharing the metadata database.
