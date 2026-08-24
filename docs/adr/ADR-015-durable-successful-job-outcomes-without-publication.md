# ADR-015: Durable successful job outcomes without publication

Status: Accepted for G04C2A implementation

## Decision

Document Studio represents an operation that completed successfully without publishing a file as a durable completion outcome, not as a failure and not as a fabricated output. Migration 6 adds the metadata-only, one-to-zero-or-one `job_completion_outcomes` table. Its allowed values are:

- `published` with no reason; or
- `no-benefit` with the exact reason `savings-threshold-not-met`.

The migration uses SQLite's null-safe `IS` comparison for the no-benefit reason so a missing reason cannot pass a `CHECK` through SQL's null result. The table stores no path, JSON, document bytes, candidate bytes or arbitrary reason. Existing jobs receive no row.

Every `JobRecord` serializes required nullable `completionKind` and `reason` fields. Legacy and current ordinary jobs remain `null`/`null`; their accepted terminal and publication evidence is unchanged. Loading an explicit outcome validates the outcome against state, output, error, resolved-name, finished-time and temporary-ownership metadata and fails closed on an impossible combination.

A no-benefit completion uses one internal, immediate SQLite transaction with an expected-version compare-and-set. It requires `verifying`, no cancellation request, no outputs, no errors, no existing outcome and no resolved output name, after the caller has proven marker-owned candidate/workspace cleanup. The transaction inserts the exact outcome and advances the job to completed with truthful complete progress and timestamps. Any failure rolls back both writes.

The generic lifecycle deliberately continues to reject `Verifying → Completed`. Only the specialized internal transaction may take that edge. Cancellation and no-benefit completion serialize on the immediate transaction: cancellation wins by recording its request before completion, or completion wins and later cancellation is too late because the job is terminal.

An internal published-completion helper applies the same outcome/state atomicity after complete durable publication evidence exists. G04C2A does not retrofit accepted G01–G04B2 operations with published outcome rows.

Completed no-benefit metadata is eligible for ordinary history deletion and retention, with foreign-key cascade deleting its outcome row. Recovery validates it, performs no output-file reconciliation or deletion, and never invents a final path. Invalid outcome combinations block recovery and deletion.

## Rationale

No benefit is a truthful successful result: the operation completed its inspection, private candidate and verification work, but the candidate did not meet the fixed value threshold. Treating it as an error would misstate the operation; creating a zero-byte or private candidate output would misstate publication and could expose data. A separate strict table keeps the distinction durable without changing `JobState`, weakening existing completion evidence or forcing a backfill.

## Consequences

- G04C2B may complete a below-threshold balanced-compression job without publishing or creating a `job_outputs` row.
- Existing completed jobs continue to serialize `completionKind: null` and `reason: null`.
- React cannot terminalize a job; no new Tauri command or capability is added.
- Older binaries fail closed on migration 6 under the accepted unknown-migration policy. Rollback requires a pre-migration database backup.
- G04C2 balanced compression remains unimplemented by this decision and requires its separate implementation and acceptance slice.
