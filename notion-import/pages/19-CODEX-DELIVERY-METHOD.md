# Codex Delivery Method

## Vertical-slice template

Each Codex task should include:

- User-visible behavior.
- Manifest/schema changes.
- Rust command/adapter.
- Job progress, cancel, verification and cleanup.
- UI state and error copy.
- Unit/integration/failure tests.
- Documentation and ADR.
- Benchmark evidence if the hot path changed.

## Prompt discipline

- Give Codex one milestone or one feature slice at a time.
- Require it to inspect the repository and propose a file-level plan before editing.
- Require exact commands and test output.
- Require screenshots for UI changes.
- Stop the task when acceptance criteria are met; do not let it expand scope.

## First real feature

Implement PDF merge after Phase 0. It exercises multiple inputs, inspection, page counts, ordering, qpdf execution, cancellation, output naming, verification, cleanup and history without complex editing coordinates.
