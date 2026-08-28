# Document Studio

**Document Studio** is a privacy-first, high-performance document workspace planned for desktop first, followed by an optional web service and mobile capture companion.

This repository contains the Document Studio product specifications and accepted local Windows foundations through the G04F1 merge on main `a664dea4755122deefceac9d6ef699a920acc398`. The desktop application is a Tauri 2, Rust, React, TypeScript and Vite workspace with metadata-only SQLite, typed IPC, durable jobs, secure temporary workspaces, progressive PDF.js viewing and verified local PDF operations. **G04E1 is an unmerged implementation candidate** on `codex/ds-g04e1-txt-to-pdf-v1`; it adds only `text.to-pdf@1.0.0` and must stop at the owner merge gate.

## Current delivery status

G01 proved the safety architecture before a production document engine was adopted. G02 now applies that architecture to a real local PDF operation:

- User documents remain in user-selected local locations or short-lived job workspaces.
- SQLite stores allow-listed metadata only. `application/history.retention_days` defaults to 30, accepts 0–365, and drives bounded startup/runtime maintenance.
- `diagnostic.copy` protocol `1.0.1` durably reserves each exact destination partial, activates a matching Windows file-identity proof before writing bytes, independently verifies it, and publishes without overwriting an existing file.
- Startup deterministically resolves every nonterminal job without pretending to resume it. Ambiguous pre-fix `1.0.0` history is preserved with a manual-inspection warning.
- Windows enforces one application instance before database or worker setup; cancellation ownership therefore remains process-local without cross-process leases.
- The webview has typed commands and progress events, a native open-dialog permission, and no shell or general filesystem capability.
- qpdf 12.3.2 is bundled from its signed official Windows archive with exact hashes, retained licenses, a zero-capability AppContainer, and owned Job Object limits.
- PDF Merge accepts 2–128 local PDFs, preserves the displayed order including intentional duplicates, creates one private snapshot per ordinal, verifies the output independently, and never overwrites by default.
- An opaque retained-handle viewer session streams 256 KiB ranges through raw bounded Tauri IPC without giving PDF.js or React a source path.
- Locally bundled PDF.js 6.2.108 uses an exact path/size/hash asset manifest and atomically staged worker assets. It renders measured-visible virtual pages/thumbnails and an ephemeral text layer/search index; PDF JavaScript, interactive form/annotation layers, attachments and external navigation are disabled.
- The shared Precision Paper organizer prepares selection, reorder, removal, output rotation and split ranges without creating history until Apply/Export.
- `pdf.extract-pages`, `pdf.remove-pages`, `pdf.reorder-pages`, `pdf.rotate-pages` and `pdf.split` 1.0.0 persist typed plans and reuse the accepted qpdf/publication/recovery boundary.
- Every split output is planned before processing. Completion requires every verified publication; partial publication is a truthful failed state that preserves already published user files.
- G04A adds accepted lossless structural PDF compression through the existing qpdf sandbox and publication boundary.
- G04B adds accepted bounded local `image.to-pdf@1.0.0` for JPEG/JPG, PNG and WebP.
- G04B2 adds `pdf.to-images@1.0.0` for ordered JPEG, PNG and lossless WebP output at 72/150/300 DPI through sequential private PDF.js canvases, authenticated raw IPC and Rust encoding.
- G04C2B is accepted on main and adds the fixed `balanced-v1` Optimize profile: safe photographic streams may be recompressed locally, every affected page is verified, and no file is created unless both 5% and 64 KiB are saved.
- G04F1 is accepted on main and adds a metadata-only preview foundation for 1–128 ordered `pdf.compress-lossless@1.0.0` jobs. It recomputes a private canonical proof and atomically creates queued/planned metadata, but has no scheduler, execution, automatic resume or batch progress engine.
- G04E1 adds one strict UTF-8 `text.to-pdf@1.0.0` candidate. A hidden Rust-owned WebView2 serves only generated HTML/CSS and three exact packaged static Noto Regular fonts from an intercepted `.invalid` origin, prints privately, normalizes through accepted qpdf 12.3.2, verifies the reopened PDF and publishes without overwrite. It adds no Office, Markdown, HTML-input, CSV/JSON/XML, batch-execution, network-server, migration or system-font path.

## Start here

1. Read `CODEX_START_HERE.md`.
2. Read `docs/00-AUDIT-AND-COMPLETION.md` and `docs/01-PRODUCT-CHARTER.md`.
3. Read `docs/23-DEVELOPMENT-SETUP.md` for locked installation, test and launch commands.
4. Read `docs/implementation-log/G01-foundation.md` for the accepted Phase 0 evidence.
5. Read `docs/implementation-log/G02-pdf-merge.md` for implementation, test, security, and performance evidence.
6. Read `docs/implementation-log/G03-viewer-core-pdf.md` for the viewer, page operations and real-browser evidence.
7. Read `docs/implementation-log/G04B-image-pdf-conversion.md`, `docs/implementation-log/G04B2-pdf-to-images.md` and ADR-013 for the independently gated conversion paths.
8. Read `docs/implementation-log/G04C2-corpus-recovery.md`, `docs/implementation-log/G04C2B-balanced-compression.md` and ADR-016 for the frozen corpus and owner-gated balanced path.
9. Read `docs/implementation-log/G04F1-batch-preview-foundation.md` and ADR-017 for the batch preview/atomic metadata boundary.
10. Read `docs/implementation-log/G04E1-txt-to-pdf.md` and ADR-018 for the hidden intercepted TXT renderer, Unicode/font gates and verification evidence.

## Validate and run on Windows

```powershell
.\.venv\Scripts\python.exe -m pip install --require-hashes --only-binary=:all: -r scripts\requirements-validation.lock.txt
npm ci --ignore-scripts
.\.venv\Scripts\python.exe -B scripts\validate_repo.py
.\.venv\Scripts\python.exe -B scripts\check_links.py
npm run typecheck
npm test
npm run test:browser --workspace @document-studio/desktop
npm run verify:g04e1 --workspace @document-studio/desktop
cargo test --workspace --all-targets --locked
npm --workspace @document-studio/desktop run tauri -- dev
```

The development window provides PDF Merge, Viewer, the accepted G04A Optimize workspace, and both local Convert directions. `diagnostic.copy` remains covered as a foundation regression operation but is not a main user workflow.

## Build philosophy

- Local-first and offline by default.
- Never overwrite originals without explicit confirmation.
- Never report success until the output has been reopened and verified.
- Prefer simple, mature engines over novel dependencies.
- Keep cloud and external AI optional and visible.
- Build every capability through the same typed operation contract.
- Performance is a product requirement, not a late optimization task.

## Package map

- `docs/` - product, system, security, UX, testing and delivery documentation.
- `apps/desktop/` - implemented Tauri + React Windows foundation.
- `apps/prototype/` - browser-openable visual prototype.
- `services/cloud-control-plane/` - optional future Go service skeleton.
- `packages/contracts/` - JSON Schema and TypeScript operation contracts.
- `packages/tokens/` - design tokens for code and Figma.
- `figma/` - Figma page/component/variable handoff.
- `models/` - approved and evaluation-only local model register.
- `diagrams/` - source and exported architecture/flow diagrams.
- `codex/prompts/` - staged implementation prompts.
- `notion-import/` - import-ready project knowledge base and attachments.
- `report/` - printable master DOCX and PDF.
- `.github/`, `CONTRIBUTING.md`, `SECURITY.md` - repository workflow, issue/PR templates and security reporting.
- `CHANGELOG.md`, `THIRD_PARTY_NOTICES.md` - release history and dependency/model adoption gate.

Current implementation version: `0.8.0-g04e1-text-to-pdf-dev`
Implementation date: `28 August 2026`

## Goal Mode build workflow

The canonical Codex execution method is documented in `docs/25-GOAL-MODE-BUILD-PLAYBOOK.md`. Start with `codex/goals/G00-readiness-audit.md` in `/plan`, then run one bounded `/goal` per milestone from `codex/goals/`.
