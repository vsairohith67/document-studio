# ADR-007: Durable publication and recovery

Status: Accepted

## Decision

Job lifecycle changes use compare-and-set database transactions against an expected state and optimistic version. A job may reach `completed` only after output verification, durable publication evidence, the no-overwrite filesystem commit, audit persistence, and confirmed temporary-data cleanup.

`diagnostic.copy` `1.0.1` uses this exact ordering:

1. Validate the UUID, names, canonical input/staging/destination, identities, space, staging size and SHA-256.
2. Choose one collision candidate and one random direct-child `.document-studio-{job-id}-{uuid}.partial` path.
3. In one immediate SQLite transaction, record the final candidate, expected evidence and exact partial reservation. Reservation is not ownership or deletion authority.
4. Create a separate same-directory guard using Windows `FILE_FLAG_DELETE_ON_CLOSE` and `create_new`. If it already exists, preserve it and release only the reservation.
5. Read the guard's Windows file identity and durably insert an activation result tied to the exact partial UUID and identity.
6. Create the exact partial path as a no-replace hard link to the guarded file object, reopen it, verify the same identity, then close the guard handle. A process exit before activation removes the guard; exit before the hard link leaves no partial; exit after the hard link leaves an identity-verifiable partial.
7. Only after activation, stream document bytes with progress/cancellation checks. Flush, `sync_all`, close, reopen, and verify size/hash.
8. Atomically let cancellation or publication commit win, then enter `publishing`.
9. Move with same-directory write-through/no-replace semantics and reopen the final for size/hash proof.
10. Record proven publication while conditionally clearing only the exact stored partial path.
11. Remove the validated UUID workspace, clear its staging path, and transition to `completed`.

Name conflicts use deterministic suffixes and are resolved again at commit time. `MAX_COLLISION_ATTEMPTS = 1000`. Before another attempt, the implementation opens only the current exact path, compares its Windows identity with the durable activation token, deletes the matching file object through that handle or proves the owned identity absent, conditionally clears that value, and atomically records the next final/partial pair. A reserved-but-unactivated path never authorizes deletion. Cleanup failure overrides collision exhaustion and leaves the job `interrupted`. Existing, final, unknown and neighboring files are never truncated or removed.

On startup, every durable nonterminal job is resolved without automatic resume. Queued work becomes failed as `JOB_WORKER_NOT_STARTED` after proven cleanup. Other pre-publication work fails after cleanup; cleanup failure remains `interrupted`. A `publishing` or `interrupted` job completes only when durable `published` evidence and the final size/hash agree; existence alone is not proof.

Version `1.0.0` predates exact destination-partial journaling. Before `verifying`, successful workspace cleanup can record `LEGACY_CLEANUP_PROVEN`. At `verifying`, `publishing`, `interrupted`, or any historical terminal state, a null path/state cannot prove an old deletion: recovery records `LEGACY_CLEANUP_UNPROVEN`, preserves unknown files, and quarantines the record from retention/history deletion. Recovery never scans a destination by filename prefix.

Cancellation is cooperative through inspection, preflight, execution, verification, and destination-partial copying. It becomes unavailable immediately before the final no-replace move and durable publication commit. A request in that window returns `CANCEL_TOO_LATE`; it never records a false cancellation.

## Rationale

The filesystem and SQLite cannot participate in one atomic transaction. The delete-on-close guard closes the create-before-activation crash window, while the activated Windows file identity closes the activation-before-partial-link window and prevents a reservation from becoming deletion authority. Journaled intent plus independently verifiable evidence makes the remaining boundary recoverable and prevents false success after a crash.

## Consequences

- Publication may perform two full streaming copies and hashes in favor of correctness.
- A post-publication database or cleanup failure remains recoverable and is not shown as completed.
- Recovery never deletes an unrecognized path and never removes a final user document.
- Schema-v3 remains unchanged; compatibility uses existing operation version, path, error and recovery-result fields.
- Rollback leaves published user outputs and metadata intact.
