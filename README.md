# Document Studio

**Document Studio** is a privacy-first, high-performance document workspace planned for desktop first, followed by an optional web service and mobile capture companion.

This repository contains the Document Studio product specifications and the accepted Windows foundation, PDF Merge, and G03 Viewer/Core PDF slices. The desktop application is a Tauri 2, Rust, React, TypeScript and Vite workspace with metadata-only SQLite, typed IPC, durable jobs, secure temporary workspaces, progressive PDF.js viewing, the `diagnostic.copy` reference operation, and accepted local PDF operations. **G03 — COMPLETE** on accepted main merge `8d6844ebdc1fd6eedf41373d53ad36eb399cc489`. **G04A — active implementation** on its approved feature branch; G04A is not accepted or complete.

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
- G04A adds only lossless structural PDF compression. Balanced/aggressive compression, image controls, batch work, conversion, OCR, repair, PDF/A, editing, redaction, cloud, accounts and AI remain later goals.

## Start here

1. Read `CODEX_START_HERE.md`.
2. Read `docs/00-AUDIT-AND-COMPLETION.md` and `docs/01-PRODUCT-CHARTER.md`.
3. Read `docs/23-DEVELOPMENT-SETUP.md` for locked installation, test and launch commands.
4. Read `docs/implementation-log/G01-foundation.md` for the accepted Phase 0 evidence.
5. Read `docs/implementation-log/G02-pdf-merge.md` for implementation, test, security, and performance evidence.
6. Read `docs/implementation-log/G03-viewer-core-pdf.md` for the viewer, page operations and real-browser evidence.

## Validate and run on Windows

```powershell
.\.venv\Scripts\python.exe -m pip install --require-hashes --only-binary=:all: -r scripts\requirements-validation.lock.txt
npm ci --ignore-scripts
.\.venv\Scripts\python.exe -B scripts\validate_repo.py
.\.venv\Scripts\python.exe -B scripts\check_links.py
npm run typecheck
npm test
npm run test:browser --workspace @document-studio/desktop
cargo test --workspace --all-targets --locked
npm --workspace @document-studio/desktop run tauri -- dev
```

The development window provides PDF Merge, Viewer, and the active G04A Optimize workspace. `diagnostic.copy` remains covered as a foundation regression operation but is not a main user workflow.

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

Current implementation version: `0.4.0-g04a-lossless-compression-dev`
Implementation date: `21 August 2026`

## Goal Mode build workflow

The canonical Codex execution method is documented in `docs/25-GOAL-MODE-BUILD-PLAYBOOK.md`. Start with `codex/goals/G00-readiness-audit.md` in `/plan`, then run one bounded `/goal` per milestone from `codex/goals/`.
