# Roadmap and Milestones

## Phase 0 - Foundation

Tauri shell, React design system, PDF viewer placeholder, job lifecycle, SQLite, secure workspace, diagnostics, recovery, prototype parity and `diagnostic.copy` reference operation.

**Exit:** a cancellable job is created, processed, verified, published, recorded and recovered correctly after forced termination.

## Phase 1 - Core PDF

Merge, split, extract, remove, reorder, rotate, linearize, PDF-to-images, images-to-PDF and naming templates.

**Exit:** golden and failure tests pass; performance budgets are measured.

G02 delivered production page-only PDF Merge. **G03 — COMPLETE** on accepted main merge `8d6844ebdc1fd6eedf41373d53ad36eb399cc489`, delivering the local viewer plus extract, page removal/reorder/rotate and explicit/fixed/every-page split planning. **G04A — COMPLETE** on accepted main merge `a27306653119e6e4fcdef162308445b78129f974`, delivering only lossless structural PDF compression. **G04B — COMPLETE** on accepted main `6940a1381822a5872f4c345cc0e5cd15b2e6294c`, delivering images-to-PDF. **G04B2 — COMPLETE** on accepted main merge `b5901a7baca58b3acb1ee00027e42b0059c59fd4`, delivering bounded PDF-to-images through the accepted PDF.js renderer. **G04C2A — COMPLETE** on accepted main `0e31c2ee75c1216c03e2d812e45da886cc9ca9e9`. **G04C2 corpus — COMPLETE** on accepted main merge `94c5bb583b7730f26635229be592cd4eeeab6a07`. **G04C2B — IMPLEMENTATION RESUMED** on its separate owner-gated branch and must not be called complete or merged by this implementation run. G04D Office conversion, G04E text/Markdown/local-HTML conversion, G04F batch execution, linearization, repair, bookmarks/outlines as an editing feature, forms, annotations and later capabilities remain deferred to separately approved slices.

## Phase 2 - Optimize, convert and protect

Compression, Office-to-PDF, text/Markdown/HTML-to-PDF, page numbers, watermark, metadata, encrypt/unlock, batch runner.

## Phase 3 - Workbench, OCR, forms and signatures

Viewer/editor interaction, overlays, crop, annotations, fill/flatten forms, signature placement, OCR in English/Hindi/Telugu.

## Phase 4 - Safety-sensitive operations

True redaction, sanitization, suspicious-content preflight, compare foundation and structured extraction.

## Phase 5 - Archival and digital signatures

PDF/A pipeline/validation, certificate signing/verification, print center.

## Phase 6 - Automation

Saved workflows, CLI, watched folders and `.dsflow` exchange.

## Phase 7 - Optional document intelligence

Local model manager, summary, Ask Document, classification and structured extraction with page citations.

## Phase 8 - Product hardening

Accessibility audit, localization, signed updates, clean-machine packaging, benchmark regression gates.

## Phase 9+ - Web/mobile/plugin ecosystem

Optional cloud control plane, browser-local router, mobile capture companion, signature requests and plugin SDK.
