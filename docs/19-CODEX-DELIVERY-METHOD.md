# Codex Delivery Method

## Approved execution method

Document Studio uses **Plan → Goal → Verify → Review → Merge**.

- Use `/plan` to inspect a milestone, identify constraints and create measurable success criteria before code changes.
- Use `/goal` after the plan is accepted so Codex can continue across the milestone until the completion gate passes or a human decision is required.
- One persistent goal equals one bounded milestone or vertical slice, not the entire 132-capability product.
- Continue fixes and testing in the same goal chat. Start a new chat and branch/worktree for the next independent goal.
- Use side chats for explanations or status recaps that should not interrupt the main goal.

Read `docs/25-GOAL-MODE-BUILD-PLAYBOOK.md` and `codex/goals/README.md` for the complete workflow and ready-to-run goals.

## Vertical-slice requirements

Each goal includes:

- User-visible outcome.
- Manifest/schema changes.
- Rust command/adapter.
- Job progress, cancel, verification, publication and cleanup.
- UI states and error language.
- Unit, integration and meaningful failure tests.
- Documentation and ADR updates.
- Benchmark evidence when the hot path changes.

## Goal discipline

- Define outcome, constraints and verification.
- Require Codex to inspect the repository before editing.
- Preserve privacy, path safety, no-overwrite and verification rules.
- Require exact commands and test output.
- Require screenshots for UI changes.
- Pause before new dependencies, architecture changes or destructive actions.
- Stop only when acceptance criteria pass or a genuine blocker requires human input.

## Sequence

G00 readiness plan → G01 foundation → G02 PDF Merge → G03 core PDF/viewer → G04 optimize/convert → G05 OCR/safety → G06 workbench/forms/signing → G07 automation/AI/hardening.
