# G01 Foundation Implementation Log

Status: Acceptance remediation implemented; independent acceptance re-review pending
Base commit: `af885a291a1d2ed05f9e10fe77540ae3588e9f7f`
Acceptance-remediation base: `ce6e096f48bf173b3fb23670abd717f362ca2c20`
Implementation branch: `feat/g01-foundation`
Target: Windows 11, Tauri 2, Rust 1.97.1, Node 24.19.0, npm 11.17.0, Python 3.12.10

## Delivered foundation

- Correct npm workspaces, a root Cargo workspace and committed npm/Cargo/Python locks.
- Strict JSON Schema, TypeScript and Rust contracts for the complete G01 lifecycle and IPC.
- Checksummed transactional SQLite migrations and a metadata-only repository with configurable application-only terminal-history retention (30 days by default).
- Windows path/file-identity validation, UUID job workspaces, exact cleanup, bounded collision-safe staged publication, deterministic non-resuming startup recovery and single-instance enforcement.
- `diagnostic.copy` `1.0.1` with durable exact destination-partial reservation plus Windows file-identity activation before bytes, streaming progress, cooperative cancellation, independent SHA-256 verification, safe audit and no-overwrite publication.
- Typed Tauri commands/events, dependency diagnostics, and a neutral design-token-driven React UI with a truthful disabled viewer placeholder.
- Real frontend, contract, migration, path-safety, copy, cancellation, failure, recovery, accessibility and leakage tests.

No production PDF engine, PDF viewer runtime, OCR, Office integration, account, cloud provider, external AI, installer, subscription or team capability was added.

## Independent acceptance remediation

The independent review of PR #4 returned **CHANGES REQUIRED**. The remediation does not claim acceptance completion; it remains subject to independent re-review.

- `diagnostic.copy` new jobs, manifest and shared fixture now use `1.0.1`; pre-fix `1.0.0` metadata is interpreted conservatively.
- The exact destination path is first committed only as a reservation. A delete-on-close `create_new` guard supplies a Windows file identity that is durably activated before a no-replace hard link creates the partial; only that matching opened identity may be deleted. Pre-existing files at the reserved or guard path are preserved.
- Collision attempts stop at 1,000, and cleanup failure takes precedence over `CollisionExhausted` or any terminal result.
- Startup resolves queued/interrupted work without resuming it. Ambiguous legacy publication-capable and terminal records receive `LEGACY_CLEANUP_UNPROVEN`, preserve unknown files and are quarantined from history deletion.
- Retention initializes/reads only `application/history.retention_days`, validates `0..=365`, and deletes at most 1,000 eligible terminal rows after recovery or a setting update.
- Official `tauri-plugin-single-instance` 2.4.3 is registered first. Forwarded inputs are ignored; the callback only restores/focuses the primary window.
- Cancellation tokens are registered before spawn; spawn failure and no-token fallback require proven cleanup and never create a false terminal result.

## Dependency and lock evidence

- `npm install --package-lock-only --ignore-scripts`: passed and created the exact npm graph.
- `npm ci --ignore-scripts`: passed from the committed lock.
- `npm audit --package-lock-only --audit-level=high`: reported zero vulnerabilities at adoption time.
- `cargo generate-lockfile` and `cargo fetch --locked`: passed for the root workspace.
- `pip-tools==7.6.0` generated `scripts/requirements-validation.lock.txt` with hashes after the project virtual environment was pinned to compatible `pip==26.1`.
- Locked Python installation with `--require-hashes --only-binary=:all:` passed.

## Original checkpoint evidence before independent re-review

| Checkpoint | Evidence | Result |
|---|---|---|
| Contracts | TypeScript schema tests and Rust golden/state tests | Passed |
| Database | 7 migration/repository/retention/prohibition tests | Passed |
| Paths/publication | 9 traversal, junction, hard-link, ownership, collision and cancellation tests | Passed |
| Copy/recovery/leakage | 13 success, failure, recovery and sentinel tests | Passed |
| Frontend | 8 React/API/token/axe tests | Passed |
| Shared contracts | 7 AJV contract tests | Passed |
| Frontend build | Strict typecheck and Vite production build | Passed |
| Rust workspace | 37 unit and integration tests | Passed |
| Rust lint | Workspace clippy with warnings denied | Passed |
| Tauri compile | Release-mode `build --no-bundle` | Passed |

## Original implementation evidence before independent re-review

The commands below completed successfully for the original implementation on 2026-08-16. They are historical evidence only and do not constitute acceptance of the remediation:

| Gate | Command | Result |
|---|---|---|
| Locked Python validation environment | `.\.venv\Scripts\python.exe -m pip install --require-hashes --only-binary=:all: -r scripts\requirements-validation.lock.txt` | Passed; PyYAML 6.0.3 already satisfied |
| Repository validator | `.\.venv\Scripts\python.exe -B scripts\validate_repo.py` | Passed; 132 feature entries verified |
| Link validator | `.\.venv\Scripts\python.exe -B scripts\check_links.py` | Passed |
| Locked npm install | `npm ci --ignore-scripts` | Passed; 122 packages audited, zero vulnerabilities |
| TypeScript | `npm run typecheck` | Passed |
| Frontend/contracts | `npm test` | Passed; 8 desktop and 7 shared-contract tests |
| Frontend build | `npm run build` | Passed |
| Rust format | `cargo fmt --all -- --check` | Passed |
| Rust lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Rust tests | `cargo test --workspace --all-targets --locked` | Passed; 37 tests |
| Tauri compile | `npm --workspace @document-studio/desktop run tauri -- build --no-bundle` | Passed; `target/release/document-studio.exe` built |
| Patch whitespace | `git diff --check` | Passed |

The Tauri command retains the approved identifier `studio.document.app` and emits only the known cross-platform advisory that `.app` is also the macOS bundle extension.

## Acceptance-remediation verification evidence

The following remediation checks passed locally on 2026-08-16. Independent acceptance re-review is still pending.

| Gate | Result |
|---|---|
| Approved dependency resolution | `cargo check --workspace` resolved `tauri-plugin-single-instance` exactly to 2.4.3 |
| Lock audit | 34 plugin/transitive packages added, none removed or version-replaced; only root and `tracing` dependency blocks changed as expected |
| Repository and links | Both Python validators passed; 132 feature entries verified |
| Frontend/contracts | Typecheck passed; 10 desktop and 7 shared-contract tests passed; Vite production build passed |
| Rust format/lint | `cargo fmt --all -- --check` and locked all-target clippy with warnings denied passed |
| Rust tests | 65 locked unit/integration tests passed, including guarded activation, pre-existing partial/guard, reservation-release, inspecting/preflight/ready recovery, and explicit legacy publishing/interrupted regressions |
| Single instance | Test-runtime two-process smoke passed; secondary exited before runtime setup |
| Production Tauri | Release-mode `build --no-bundle` passed without `test-runtime` |
| Diff safety | `git diff --check` passed; migrations, capabilities and npm lock remained unchanged |

The Python hash-locked install was not repeated because its locked environment was already present and no Python dependency changed. `npm ci --ignore-scripts` was repeated from the unchanged lockfile and audited 122 packages with zero vulnerabilities. The unrelated GitHub Actions Node runtime warning remains non-blocking and action pins were not changed.

## Windows interactive evidence

- `npm --workspace @document-studio/desktop run tauri -- dev` launched the native Windows application successfully.
- A 2.6 KB contract fixture completed twice. The second run published `foundation-contracts-copy (1).json`, proving deterministic collision handling without replacing the first output.
- A 737.1 MB local build artifact was cancelled during destination publication. The UI reached `cancelled`, SQLite recorded a terminal cancellation, the job workspace was removed, and the destination retained only the two previously verified 2.6 KB outputs with no partial file.
- The first cancellation attempt exposed a race where the job entered `publishing` before the cancellable destination copy. The fix keeps that copy in `verifying`, establishes an atomic publication-commit boundary, and adds focused registry and destination-copy regression tests.
- Final race review added concurrent cancel/commit proof and a service-level commit-time collision test. Exactly one cancel/commit outcome wins, a competing file is preserved, and publication retries the next suffix with updated durable intent.
- Startup recovery reconciled the pre-fix stuck `publishing` record to `interrupted` instead of reporting false success.
- Keyboard-only Tab navigation displayed a clear focus outline on native controls.
- A second process-local launch used `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--force-device-scale-factor=2 --force-high-contrast --force-prefers-reduced-motion'`. The 200% scaled, forced-colors layout retained readable content, keyboard controls and vertical scrolling without horizontal clipping; no Windows system preference was changed.
- The successful development-launch screenshot is [g01-tauri-dev-launch.png](assets/g01-tauri-dev-launch.png).
- The development process was stopped with `Ctrl+C`; the expected Windows console-control exit is not an application build or test failure.

## Security and privacy evidence

- The final source-backed security diff review covered all 44 changed source/configuration items across eight security surfaces and produced no reportable findings.
- SQLite migrations contain no BLOB column and repository tests reject prohibited settings/content.
- Sentinel document content and a sentinel password were absent from SQLite, progress events, errors and dependency diagnostics.
- The Tauri capability contains only `core:default` and `dialog:allow-open`.
- No HTTP, shell or general filesystem plugin is adopted.
- Existing sources and outputs are opened without truncation; publication uses a create-new delete-on-close guard, an identity-checked no-replace partial hard link, and a no-replace final move.
- Cleanup validates a UUID path and ownership marker below the canonical application workspace root.

## Known limits

- G01 copies bytes but does not parse, render or transform a PDF.
- Automated axe checks run in jsdom. Native focus, 200% effective scaling, forced colors and reduced-motion preference were additionally checked in the Tauri smoke; a full assistive-technology certification remains a later release activity.
- Publication performs additional full streaming reads/hashes to prioritize correctness over speed.
- Installer packaging, signing and distribution are outside G01.
- Pre-fix `1.0.0` development databases can contain cleanup ambiguity that software cannot resolve without guessing. A metadata reset requires separate approval and manual inspection of affected destinations; it is not part of this remediation.

## Rollback

Use an approved `git revert` after a G01 commit; do not reset or broadly clean the repository. Restore dependency manifests and their lockfiles together. Migrations remain append-only. Rollback never deletes user-published output or the metadata database. Cleanup may remove only an exact recorded destination partial whose opened Windows identity matches its durable activation token, or a validated owned UUID workspace.

The final Git cleanliness proof is intentionally deferred until the separately approved G01 commit is created. No push is part of G01.
