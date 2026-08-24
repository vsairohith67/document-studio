# Changelog

All notable planning-package and software releases are recorded here.

## 0.5.1-g04b2-pdf-to-images - unreleased

- Added `pdf.to-images@1.0.0` using the accepted local PDF.js 6.2.108 renderer and existing `image` 0.25.10 encoder; no new native renderer, runtime download, network or capability was introduced.
- Added ordered 1-128 page export to PNG, fixed-quality JPEG and lossless WebP at exactly 72, 150 or 300 DPI, with per-page 8,192-axis/16,777,216-pixel caps and a 67,108,864-pixel aggregate budget.
- Added sequential private-canvas rendering, authenticated one-use binary RGBA IPC, strict Rust size/alpha/sequence/ownership checks, exact/tolerant visual verification and durable collision-safe multi-output publication.
- Enabled the accessible PDF-to-images Convert flow with shared viewer thumbnails, output-name preview, keyboard ordering, per-output progress, cancellation and truthful partial-publication reporting.
- Superseded the ADR-013 renderer blocker, added a G04B2 boundary verifier and focused contract, native, frontend and existing-framework PDF.js browser evidence. G04C and G05 remain excluded.

## 0.5.0-g04b-image-pdf-conversion - 2026-08-22

- Added the independently gated `image.to-pdf@1.0.0` implementation for ordered, content-sniffed JPEG/PNG/WebP inputs with fixed axis, pixel, selected-total, byte and decoder-allocation limits.
- Added deterministic EXIF-orientation, alpha soft-mask and ICC-warning behavior; one image per zero-margin PDF page; source immutability proof; qpdf structural/page/encryption verification; and durable no-overwrite publication.
- Added migration 5 for canonical hashed operation settings and sanitized job warnings without changing accepted G03 page plans or storing document content/raw paths.
- Added the accessible two-direction Convert workspace with the PDF-to-images direction reserved for its separately gated implementation.
- Added ADR-013, native/frontend/database tests and an exact G04B boundary verifier. G04C-G04F production work is excluded.
- Accepted and merged G04B to main at `6940a1381822a5872f4c345cc0e5cd15b2e6294c`.

## 0.4.0-g04a-lossless-compression - 2026-08-22

- Added accepted `pdf.compress-lossless@1.0.0` through the existing qpdf 12.3.2 sandbox, independent verification and durable publication boundary.
- Added the bounded Optimize UI, structural/visual/source-immutability evidence and exact G04A boundary verification.
- Accepted and merged G04A to main at `a27306653119e6e4fcdef162308445b78129f974`.

## 0.3.0-g03-viewer-core-pdf - unreleased

- Added opaque retained-handle PDF viewer sessions, backend open/drop handling, raw bounded range IPC and explicit close/source-change behavior without exposing source paths.
- Pinned `pdfjs-dist` 6.2.108 and `@tanstack/react-virtual` 3.14.9; packaged the matching local worker/CMaps/fonts/ICC/WASM and added the restricted viewer CSP/navigation policy.
- Added the progressive Precision Paper viewer: virtual pages/thumbnails, navigation, zoom/fit, temporary view rotation, text selection, bounded incremental Unicode search and truthful password/image-only/error states.
- Added migration 4 for canonical hashed 64 KiB page-plan envelopes and generalized expected-output/publication/recovery records without storing document content, text or thumbnails.
- Added `pdf.extract-pages`, `pdf.remove-pages`, `pdf.reorder-pages`, `pdf.rotate-pages` and `pdf.split` 1.0.0 through bundled qpdf 12.3.2 with strict independent verification and truthful partial publication.
- Added Vitest, deterministic qpdf integration, real Chromium PDF.js, real WebView2 raw IPC, security/leakage, recovery and measured 100/1,000-page evidence.
- Redacted every G03 durable-job source, destination, staging, partial and final path at IPC serialization while retaining trusted recovery metadata and unchanged G01/G02 behavior.
- Remediated independent review findings with transactional candidate replacement, exact atomic PDF.js asset staging, strict canvas/visibility bounds, runtime job/plan matching, compact command-budgeted qpdf argv, Alt+Arrow group reorder, fail-closed production WebView2 filtering and deterministic Close/password-Cancel focus. PR #6 remains draft and pending independent re-review.

## 0.2.0-g02-pdf-merge - unreleased

- Added the `pdf.merge` 1.0.0 contract foundation for 2–128 ordered local PDFs without a database migration or Tauri capability expansion.
- Bundled the signed qpdf 12.3.2 MSVC64 runtime with exact file hashes, provenance, Apache-2.0 license materials and reproducible acquisition/verification scripts.
- Proved fixed-name zero-capability AppContainer launch, one-process/2 GiB/kill-on-close Job Object limits, filesystem and loopback denial, owned termination, and the exact production merge argument vector.
- Added the production merge worker: per-ordinal snapshots, identity-deduplicated bounded preflight, direct sandboxed qpdf execution, bounded process output, owned cancellation, independent output verification, G01 publication, and non-resuming recovery.
- Added the accessible Precision Paper PDF Merge workspace with multi-select/drop, exact ordering, keyboard and pointer reorder, remove, destination/name validation, truthful progress/cancellation, and verified results.
- Added generated-fixture regression coverage and bounded Windows performance evidence. The planned approximately 1 GiB byte-volume corpus remains a pre-release benchmark follow-up.

## 0.1.0-g01-foundation - 2026-08-16

- Remediated acceptance findings with `diagnostic.copy` protocol `1.0.1`, durable exact destination-partial reservation plus file-identity activation, bounded collision recovery, deterministic non-resuming startup recovery, application-only runtime retention, conservative `1.0.0` quarantine, and Windows single-instance enforcement.
- Added reproducible npm, Cargo and Python validation workspaces and lockfiles.
- Added strict TypeScript, JSON Schema and Rust contracts for jobs, stages, settings, dependencies, errors, IPC and progress.
- Added a metadata-only SQLite repository with transactional checksummed migrations and 30-day terminal-history retention.
- Added secure Windows path checks, private job workspaces, no-overwrite publication, cancellation and startup recovery.
- Added the SHA-256-verified `diagnostic.copy` reference operation without adding a production PDF engine.
- Added a neutral, design-token-driven Tauri/React foundation UI and real frontend, Rust, migration, path-safety, recovery and leakage tests.
- Pinned CI actions and toolchains and added locked validation, formatting, clippy, tests, frontend build and Tauri no-bundle compilation.

## 2.0.1-final-recheck - 2026-07-22

- Established **Document Studio** as the canonical product name.
- Re-audited product scope into one 132-capability catalogue.
- Added the desktop-first platform strategy, performance plan, operation contract, database/API/security designs and staged roadmap.
- Added the Tauri 2 + React + TypeScript + Vite starter, static UI prototype, design tokens, Figma handoff, model registry and Codex prompt pack.
- Added Notion import, whiteboard exports, historical-research archive and print-ready master DOCX/PDF.
- Added validation evidence and an explicit boundary between local deliverables and connected-app publication.

## 1.0.0 - archived

- Earlier personal product specification under a retired working name. Retained only in `notion-import/attachments/archive/` for traceability.

## 2.1.0-goal-mode — 28 July 2026

- Adopted Plan → Goal → Verify → Review → Merge as the canonical Codex workflow.
- Added bounded Goal Mode files G00–G07.
- Added the Goal Mode Build Playbook and Notion-ready page.
- Added goal lifecycle whiteboard exports.
- Updated CODEX_START_HERE.md, delivery method and project hub guidance.
