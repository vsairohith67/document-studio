# ADR-007: Durable publication and recovery

Status: Accepted

## Decision

Job lifecycle changes use compare-and-set database transactions against an expected state and optimistic version. A job may reach `completed` only after output verification, durable publication evidence, the no-overwrite filesystem commit, audit persistence, and confirmed temporary-data cleanup.

Publication first records intent and verified staging size/hash. It then copies into a randomly named destination-local partial using create-new semantics, flushes and independently verifies that partial, and performs a same-directory no-replace move to the resolved final name. Name conflicts use deterministic suffixes and are resolved again at commit time. Existing files are never truncated or replaced.

On startup, ordinary in-flight jobs become `interrupted` and their owned temporary files are reconciled. A persisted `publishing` job may be finalized only when its publication intent, recorded verified size/hash, audit evidence, and the final file all agree. An output file's mere existence is not proof of success. Cleanup or audit ambiguity remains `interrupted` for explicit recovery.

Cancellation is cooperative through inspection, preflight, execution, verification, and destination-partial copying. It becomes unavailable immediately before the final no-replace move and durable publication commit. A request in that window returns `CANCEL_TOO_LATE`; it never records a false cancellation.

## Rationale

The filesystem and SQLite cannot participate in one atomic transaction. A journaled intent plus independently verifiable evidence makes the unavoidable boundary recoverable and prevents false success after a crash.

## Consequences

- Publication may perform two full streaming copies and hashes in favor of correctness.
- A post-publication database or cleanup failure remains recoverable and is not shown as completed.
- Recovery never deletes an unrecognized path and never removes a final user document.
- Rollback leaves published user outputs and metadata intact.
