# G04C2A Durable Completion Outcomes

## Scope

This vertical slice adds only the generic durable outcome foundation required by a future balanced-compression operation. It adds migration 6, equivalent Rust/TypeScript/JSON contracts, one repository loader, internal atomic completion helpers, recovery/retention/history validation and neutral generic UI rendering.

It does not add `pdf.compress-balanced`, qpdf mutation, image recompression, visual metrics, a frontend control, dependency, Tauri command, capability or network path. G04C2 production remains unimplemented until G04C2B.

## Invariants

- Existing jobs have no outcome row and serialize required `completionKind: null` and `reason: null` fields.
- Published outcomes require fully published output evidence; no-benefit requires zero outputs, zero errors, no resolved output name and exact reason `savings-threshold-not-met`.
- No-benefit completion is one immediate expected-version transaction after caller-proven marker-owned cleanup.
- Cancellation and completion have exactly one winner. Generic `Verifying → Completed` remains illegal.
- No fake output or `job_outputs` row represents no benefit.
- Recovery never resumes work or performs an output-file operation for a valid no-benefit job.
- History and retention may delete valid metadata and rely on foreign-key cascade; invalid outcomes fail closed.

## Validation

Focused contract, UI, repository, migration, recovery, concurrency and fault-injection tests are part of this slice. Repository-required validators, full frontend/native tests, warning-denied clippy, production build and Tauri no-bundle acceptance must all pass at the exact PR head before the slice can be accepted.

The pre-PR independent review found and repaired two medium integrity gaps before publication: explicit outcomes now reject non-null cancellation metadata at load time, and published outcomes require recorded `verified_at` evidence at both completion and load time. Focused negative tests isolate both cases. The repair re-review found no remaining Blocker, High or Medium issue.

The repaired worktree passed repository validation and links, TypeScript typecheck, 98 desktop and 27 shared-contract tests, all Rust workspace tests (with only the two documented manual performance cases ignored), warning-denied clippy, all G03/G04A/G04B/G04B2/G04C2A boundary verifiers, nine PDF.js asset cases, the native WebView2 smoke, 22 Playwright cases and the Tauri release no-bundle build.
