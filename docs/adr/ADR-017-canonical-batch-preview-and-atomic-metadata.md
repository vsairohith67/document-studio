# ADR-017: Canonical batch preview and atomic metadata creation

Status: Accepted for G04F1 implementation

## Decision

G04F1 introduces only a preview and metadata foundation for `pdf.compress-lossless@1.0.0`. The request contains 1–128 ordered local PDF paths, one restricted naming template, an existing local destination and exactly empty settings. The grammar requires exactly one `{stem}`, permits at most one one-based, three-digit `{index}`, and uses `{{`/`}}` escapes. Every other token, operation or version is rejected, including balanced compression, image conversion, merge, split, viewer/page plans and unknown values.

Rust opens each source read-only while denying write/delete sharing, verifies PDF name and magic, and records its ordered file identity, exact byte size, nanosecond modification time, SHA-256 and a private SHA-256 of the exact selected canonical path. The path digest makes same-identity hard-link aliases stale without exposing the path through IPC or canonical preview storage. It verifies the packaged qpdf runtime and any existing private runtime cache without materializing or repairing that cache. The destination is represented in the private canonical preview only by its directory identity, and a read-only directory handle proves the minimum add-file access right without creating a probe file. That retained handle denies delete sharing until atomic metadata commit. Existing destination entry names of every type are enumerated once and compared using Windows ordinal-ignore-case semantics. The IPC response exposes only safe display names, planned output names, collision indexes, source sizes, estimates and proof fields. It contains no path, file identity, source hash or modification time. The private envelope also contains the SHA-256 of exact canonical `{}` settings bytes, the template, collision decisions and a conservative disk estimate: destination requirements are summed, ordinary-job workspace requirements use the serial peak, and both are added when the volumes are shared. Compact `serde_json` struct serialization is the canonical UTF-8 representation; its exact bytes are bounded to 262,144 and hashed with SHA-256. JSON Schema cannot express the Windows 255 UTF-16-code-unit component boundary because its `maxLength` counts Unicode code points; Rust remains authoritative.

Creation accepts the same request plus the preview SHA-256 and optimistic version. Rust reopens and rehashes every source, rechecks destination identity and the complete naming/collision result, recomputes the exact canonical envelope, requires equal proofs and checks current free space. Only then does one immediate SQLite transaction enforce the one-live-plan/CAS gate and insert a batch record, all ordinary queued lossless-compression child jobs, inputs, planned outputs, canonical operation specs and ordered links. The preview payload is not stored. Every mismatch returns `BATCH_PLAN_STALE`; a stale value, collision change, low-space result or any insert failure creates no batch and no child.

Batch progress is a count of settled children. It never combines child byte/item/step percentages. Explicit `no-benefit` completion remains successful and settled but is neither failed nor published. Legacy successful children with complete publication evidence remain published. An interrupted child is visible but not settled.

G04F1 has no scheduler and never starts a child. Startup recovery excludes only unstarted queued batch children; any future active ordinary child remains subject to the existing evidence-based non-resuming recovery. No restart path may blindly resume document processing.

## Consequences

- Migration 8 follows the accepted G04C2B migration 7 without changing migrations 1–7.
- SQLite stores metadata required for local recovery and future coordination, not document bytes, extracted content or the canonical preview payload.
- The React preview can display names, collision choices and estimates, but it receives no source/destination path inside the preview response and never invokes the ordinary worker command.
- Later scheduling, batch cancellation, active-child transitions, batch deletion/retention and operation opt-ins require separate authorized slices.
