# Document Studio Codex Goals

Use `/plan` to shape a milestone when needed, then start a persistent `/goal` using one of the files in this folder.

Rules:

1. One goal equals one bounded milestone or vertical slice.
2. Keep fixes and verification for that goal in the same chat.
3. Use a new branch/worktree and chat for the next independent goal.
4. Never run two goals against the same checkout.
5. The goal must state outcome, constraints and verification.
6. Goal Mode does not override AGENTS.md, approvals, sandboxing, ADR gates or privacy defaults.

Sequence:

- G00 readiness audit — plan only, no edits.
- G01 Phase 0 foundation.
- G02 PDF Merge vertical slice.
- G03 viewer virtualization and core PDF operations.
- G04 optimization and conversion.
- G05 OCR, protection, sanitization and true redaction.
- G06 workbench, forms and signing.
- G07 automation, optional AI and product hardening.
