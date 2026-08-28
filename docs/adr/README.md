# Architecture Decision Records

| ADR | Decision | Status |
|---|---|---|
| [ADR-001](ADR-001-desktop-first.md) | Ship Windows desktop first | Accepted |
| [ADR-002](ADR-002-rust-desktop-orchestrator.md) | Use the Tauri Rust core as desktop orchestrator | Accepted |
| [ADR-003](ADR-003-local-first-ai-optional.md) | Keep AI optional and provider-neutral | Accepted |
| [ADR-004](ADR-004-output-verification.md) | Require output verification before publication | Accepted |
| [ADR-005](ADR-005-storage-ownership-and-provider-persistence.md) | Keep documents user-owned and provider persistence deferred | Accepted |
| [ADR-006](ADR-006-foundation-dependencies-and-sqlite.md) | Adopt reviewed foundation dependencies and bundled SQLite | Accepted |
| [ADR-007](ADR-007-durable-publication-and-recovery.md) | Require journaled no-overwrite publication and evidence-based recovery | Accepted |
| [ADR-008](ADR-008-single-instance-cancellation.md) | Enforce one Windows application process for cancellation ownership | Accepted |
| [ADR-009](ADR-009-qpdf-and-production-pdf-merge.md) | Bundle qpdf for sandboxed, verified, page-only PDF Merge | Accepted; implemented in G02 |
| [ADR-010](ADR-010-pdfjs-local-rendering-security.md) | Pin and locally package PDF.js with a reduced viewer surface | Accepted; implemented in G03 |
| [ADR-011](ADR-011-opaque-viewer-document-sessions.md) | Stream local PDFs through opaque retained-handle sessions | Accepted; implemented in G03 |
| [ADR-012](ADR-012-versioned-page-plans-and-multi-output-publication.md) | Persist typed page plans and represent partial publication truthfully | Accepted; implemented in G03 |
| [ADR-013](ADR-013-g04b-image-pdf-conversion-dependencies.md) | Adopt bounded image/PDF conversion engines and reuse accepted PDF.js for authenticated sequential raster export | Images-to-PDF accepted; G04B2 decision approved |
| [ADR-014](ADR-014-webview2-startup-environment-policy.md) | Remove inherited WebView2 controls before Tauri startup and retain only the app-owned profile/test builder boundaries | Accepted for SEC1C implementation |
| [ADR-015](ADR-015-durable-successful-job-outcomes-without-publication.md) | Represent successful no-publication completion with strict durable outcome metadata | Accepted for G04C2A implementation |
| [ADR-016](ADR-016-conservative-object-aware-balanced-compression.md) | Use a fixed object-aware qpdf/PDF.js pipeline for conservative balanced compression | Accepted for G04C2B implementation; owner merge pending |
| [ADR-017](ADR-017-canonical-batch-preview-and-atomic-metadata.md) | Use a private canonical preview proof and one atomic metadata transaction for batch planning | Accepted for G04F1 implementation |
| [ADR-018](ADR-018-hidden-webview2-text-pdf-renderer.md) | Use typed Windows/WebView2 projections for a hidden intercepted TXT-to-PDF renderer | Accepted for G04E1 implementation |

Create a new ADR for decisions that are expensive to reverse, affect security/privacy, change the operation contract, add a production dependency/model or alter the platform sequence. Never rewrite an accepted ADR to hide history; supersede it with a new record.
