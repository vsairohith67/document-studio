# Codex Start Here

## Purpose

Use this file as the first message/context document when opening the repository in Codex. Codex should implement **one verified vertical slice at a time**. It must not attempt to build the entire feature catalogue in a single run.

## Canonical architecture

- Desktop shell: Tauri 2.
- UI: React 19.2 + TypeScript + Vite 8.1.
- Desktop orchestration: Rust in `src-tauri`.
- Viewer: PDF.js.
- Local metadata: SQLite through a restricted Tauri SQL capability or a Rust repository layer.
- Structural PDF operations: qpdf adapter.
- Image operations: libvips adapter.
- OCR: OCRmyPDF + Tesseract language packs.
- Office conversion: LibreOffice headless; Gotenberg only for optional server deployment.
- Optional document intelligence: Docling and selected Hugging Face models, disabled until their evaluation gates pass.
- Future web control plane: Go; do not add it to the desktop critical path.

## Non-negotiable engineering rules

1. Preserve originals by default.
2. Use validated argument arrays; never construct shell command strings from user input.
3. Isolate each job in a private working directory.
4. Emit structured progress and support cancellation.
5. Stage outputs, reopen them, run operation-specific verification, then publish atomically.
6. Clean temporary data on success, failure, cancel and crash recovery.
7. Record actionable diagnostics without document contents, passwords, keys or extracted text.
8. Add unit, integration and failure tests with every operation.
9. Update the relevant docs and ADR in the same change.
10. Stop if an operation cannot be made truthful. A black rectangle is not redaction; an exit code is not verification.

## First implementation sequence

1. Run the repository checks.
2. Scaffold the Tauri desktop app from `apps/desktop`.
3. Implement the typed job state machine and SQLite migrations.
4. Implement `diagnostic.copy` as the reference operation.
5. Add dependency diagnostics and startup recovery.
6. Pass Phase 0 acceptance tests.
7. Implement PDF merge as the first real operation.

## Expected Codex response after each slice

- Files changed.
- Commands run.
- Test results.
- Screenshots or rendered evidence when UI changed.
- Known limitations.
- Exact next slice recommendation.
