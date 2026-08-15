# Codex Prompt 01 - Foundation

Read `AGENTS.md`, `CODEX_START_HERE.md`, `docs/09-SYSTEM-ARCHITECTURE.md`, `docs/10-OPERATION-CONTRACT.md` and `docs/22-IMPLEMENTATION-READINESS-CHECKLIST.md` before editing.

Implement Phase 0 only:

- Make the Tauri/React shell run.
- Implement design-token consumption and Home layout parity with the prototype.
- Add SQLite migrations for jobs, inputs, outputs, presets, workflows, dependencies and settings.
- Implement the typed job state machine.
- Implement secure per-job workspaces and startup reconciliation.
- Implement `diagnostic.copy` through inspect, preflight, estimate, execute, cancel, verify, publish, audit and cleanup.
- Add dependency diagnostics.
- Add tests for completion, cancel, failure, crash reconciliation and path safety.

Before editing, give a file-by-file plan. After editing, run formatting, typecheck, Rust/unit/integration tests and a development smoke test. Stop when Phase 0 acceptance criteria pass.
