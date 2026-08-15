# Codex Start Here

## Purpose

Document Studio is built through **persistent, bounded Goal Mode milestones**, not one giant prompt and not a stream of disconnected micro-prompts.

1. Read `AGENTS.md`, `FINAL_RECHECK.md`, `docs/19-CODEX-DELIVERY-METHOD.md`, `docs/25-GOAL-MODE-BUILD-PLAYBOOK.md` and `codex/goals/README.md`.
2. Run G00 in `/plan` without edits.
3. After the readiness plan is accepted, start G01 with `/goal`.
4. Keep all implementation, fixes and acceptance work for a goal in the same chat.
5. Start the next independent goal in a new branch/worktree and chat.

## Canonical architecture

- Desktop shell: Tauri 2.
- UI: React + TypeScript + Vite.
- Desktop orchestration: Rust.
- Viewer: PDF.js.
- Local metadata: SQLite through a Rust repository layer or tightly restricted capability.
- Structural PDF operations: qpdf adapter.
- Image operations: libvips adapter.
- OCR: OCRmyPDF + Tesseract language packs.
- Office conversion: isolated LibreOffice headless worker.
- Optional document intelligence: disabled until model evaluation gates pass.
- Future web control plane: separate and never required by the desktop critical path.

## Non-negotiable rules

1. Preserve originals by default.
2. Use validated typed commands and argument arrays; never construct shell command strings from user input.
3. Isolate each job in a private working directory.
4. Emit structured, truthful progress and support cancellation.
5. Stage outputs, reopen them, verify operation-specific invariants, then publish atomically where possible.
6. Clean temporary data on success, failure, cancel and crash recovery.
7. Record diagnostics without document contents, passwords, keys or extracted text.
8. Add unit, integration and failure tests with every operation.
9. Update relevant documentation and ADRs in the same change.
10. Stop if an operation cannot be made truthful.

## Required goal sequence

- G00 Readiness audit — `/plan`, no edits.
- G01 Phase 0 foundation — `/goal`.
- G02 PDF Merge vertical slice — `/goal`.
- G03 Core PDF and viewer — `/goal`.
- G04 Optimize and convert — `/goal`.
- G05 OCR and safety — `/goal`.
- G06 Workbench, forms and signing — `/goal`.
- G07 Automation, optional AI and hardening — `/goal`.

## Evidence after each goal

- Files changed.
- Commands run and exit results.
- Test and benchmark evidence.
- Screenshots/rendered evidence for UI work.
- Known limitations and unresolved decisions.
- Rollback notes.
- Recommended next goal.
