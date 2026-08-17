# G02 — Production PDF Merge

Status: G02 READY TO STAGE on `feat/g02-pdf-merge`; implementation and local acceptance evidence are complete, with no staging, commit, push or release.

Outcome: add and inspect 2–128 local PDFs, preserve the exact displayed order including intentional duplicates, reorder/remove entries, choose a safe output name and destination, run a truthful cancellable merge, independently verify one page-only PDF, and publish it without overwrite through the G01 lifecycle.

Constraints: use `pdf.merge` `1.0.0`, the shared job engine and metadata-only storage; checksum- and signature-govern bundled qpdf 12.3.2; direct argument arrays only; stable fixed-name zero-capability AppContainer plus owned Job Object; one physical ASCII snapshot per persisted ordinal; no source mutation; no silent repair, cloud, unsandboxed fallback, page ranges, viewer, thumbnails, or overwrite. The production builder excludes `--deterministic-id`. No migration or Tauri capability expansion is planned.

Verification: one-to-one UI/ordinal/snapshot/`--file` order, strict check exit rules, encryption exit rules, expected summed page count and semantic source order, staging/final SHA-256 equality, corrupt/encrypted/zero-page/changed input errors, cancellation leaves no unknown partial, naming collisions never overwrite, deterministic non-resuming recovery, privacy leakage checks, generated fixtures, measured bounded performance, and keyboard/accessibility review.
