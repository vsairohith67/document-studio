# G01 — Phase 0 Foundation

Bring Document Studio Phase 0 to a complete, runnable, tested state on Windows.

Outcome: Tauri/React shell, committed design tokens, SQLite repository, typed durable job lifecycle, secure per-job workspaces, staged and verified publication, diagnostic.copy, truthful progress/cancel, dependency diagnostics, cleanup and startup recovery.

Constraints: obey AGENTS.md and ADRs; preserve architecture; local and network-free by default; never overwrite inputs; no qpdf/OCR/Office/AI/cloud scope; pause before new dependencies or contract/architecture changes.

Verification: validators, link checks, npm install/lockfile, typecheck/tests, cargo fmt/clippy/test, Tauri smoke, diagnostic.copy success/cancel/failure/path/crash tests, accessibility smoke, implementation log and screenshots. Stop only when Phase 0 passes or a genuine blocker needs input.
