# G04F1 Batch Preview and Contract Foundation

## Scope

This vertical slice adds a versioned, sanitized batch preview response and stale-proof atomic metadata creation for exactly `pdf.compress-lossless@1.0.0`. It accepts 1–128 ordered PDFs, exact empty settings, the restricted `{stem}`/optional `{index}` naming grammar and an existing local destination directory.

It does not add a scheduler, execute a child, start a worker, resume document processing, add batch cancellation, or enable another operation/version. Balanced compression, image conversion, merge, split, viewer/page plans and unknown operations remain ineligible.

## Reconciliation and migration order

The replay branch starts at accepted post-G04C2B main `198f29fae206b21dbd9d8a9b4689f5865b94420d`. Live migrations 1–7 were enumerated from files, registration and tests before adding `0008_batch_preview_foundation.sql`; migrations 1–7 remain byte-identical.

Batch child loading preserves `completionKind` and `reason`. A completed `no-benefit` child is successful and settled, but is neither failed nor published. This does not make balanced compression eligible for G04F1.

## Safety and privacy invariants

- Preview canonicalizes the exact operation/version, exact `{}` settings hash, ordered private source fingerprints, destination identity, naming template, Windows ordinal-ignore-case collision plan, optimistic version and checked disk estimate, then hashes the exact compact UTF-8 bytes. The returned rows omit paths, identities, hashes and modification times.
- Creation reopens every source with restrictive sharing, recomputes the entire preview, requires the exact preview hash/version, rechecks collisions and current free space, and keeps source handles open through commit. Every proof mismatch is typed `BATCH_PLAN_STALE`.
- One immediate SQLite transaction enforces one live plan and inserts the batch, all ordinary queued child jobs, inputs, planned outputs, operation specs and ordered links. Any mismatch or insert failure leaves zero new rows.
- Preview bytes and source/destination paths are not logged or synchronized. The canonical preview payload is not stored.
- No child is dispatched. Queued unstarted batch metadata survives recovery; active ordinary jobs remain evidence-reconciled without blind resume.
- Progress uses settled-child counts only and never combines byte, item or step percentages.

## Validation

Focused local evidence is recorded in the task report and PR body after commands finish. The implementation log does not pre-claim complete CI or heavy Windows gates.

The focused G04F1 cases cover eligibility rejection, canonical determinism, sanitized output, exact and malformed hash/version mismatch, source/destination/collision staleness, same-name hard-link alias replay, destination rename denial while the retained permission handle is live, all-entry ordinal-ignore-case collision planning, exact disk threshold and overflow, every insert-boundary rollback, explicit-history and retention preservation, 128-input acceptance, 129-input rejection, naming grammar/escaping, authoritative UTF-16 component limits, Windows case decisions and truthful published/no-benefit aggregation.

## Release state

Implementation stops at the owner merge gate. This branch is not merged, tagged, released or deployed, and no batch scheduling or document processing is activated.
