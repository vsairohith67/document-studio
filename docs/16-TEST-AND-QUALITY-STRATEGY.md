# Test and Quality Strategy

## Layers

- Unit tests: schemas, range parsing, naming, path validation, job transitions, settings migrations.
- Adapter integration: known input -> operation -> verified output.
- Golden visual tests: render selected outputs and compare with tolerance.
- Cross-renderer tests: PDF.js plus at least one independent desktop/PDFium renderer.
- Security tests: malformed files, traversal, command injection, archive bombs, cancellation and secret logging.
- Recovery tests: kill the process during every stage and relaunch.
- Performance tests: named fixtures and thresholds by stage.
- Accessibility tests: keyboard, focus, screen-reader names, 200% zoom, high contrast and reduced motion.
- Packaging tests: clean Windows/macOS VM, upgrades, missing dependencies and uninstall.

## Definition of done for a feature

1. Manifest and typed settings complete.
2. UI explains accepted inputs, locality and risky choices.
3. Preflight catches predictable failures.
4. Progress and cancellation work.
5. Output is staged and verified.
6. Original remains unchanged by default.
7. Temporary files are cleaned in all terminal paths.
8. Success and at least three meaningful failure tests exist.
9. History is useful and does not leak content/secrets.
10. User documentation and performance evidence are updated.

## G01 acceptance matrix

The foundation has executable TypeScript/JSON Schema, React/jsdom/axe and Rust unit/integration tests. The G01 suite covers:

- valid and invalid contracts plus cross-language golden fixtures;
- every legal lifecycle edge and rejected skipped/backward/terminal transitions;
- fresh/idempotent/failed/checksum-mismatch migrations, application-only retention validation, both sides of injected cutoffs, zero retention, legacy quarantine and the 1,000-row maintenance bound;
- success, zero-byte files, chunked progress, queued/running cancellation and too-late cancellation semantics;
- traversal, unsafe Windows names, junction/reparse escape and hard-link input/output collisions;
- reservation before creation, guarded file-identity activation before bytes, deterministic pre-existing-file/create-new collisions, reservation-release failure, both reservation/activation crash windows, termination during copy, exact deletion failure/restart cleanup, neighboring-file preservation, existing-output suffixing, 1,000-attempt collision exhaustion, write failures and verification mismatches;
- every nonterminal startup state including inspecting/preflight/ready, worker-spawn failure, no-token cancellation fallback, explicit legacy `1.0.0` verifying/publishing/interrupted compatibility boundaries, publishing evidence and false-success prevention;
- a Windows two-process smoke proving the secondary exits before runtime/database setup;
- metadata, event, error and dependency-diagnostic content/secret leakage;
- typed command names/payloads, progress sequence-gap reconciliation, neutral UI copy, token application, keyboard controls and axe smoke.

Automated jsdom accessibility checks do not prove color contrast or native WebView behavior. Final G01 acceptance therefore also checks visible focus, keyboard order, 200% zoom, Windows high contrast and reduced motion in the launched Tauri application.

## G02 regression matrix

Generated local fixtures cover two and many inputs, semantic order, repeated input and hard-link alias, Unicode and paths longer than 260 characters, malformed/recovery-required/encrypted/zero-page files, busy or changed sources, destination/input aliases, no-overwrite collision, low space, corrupt staging, cleanup failure, and source immutability. No third-party PDF fixture is committed.

Process tests prove the exact production argv without `--deterministic-id`, fixed zero-capability profile, filesystem and loopback denial, bounded 64 KiB stdout/stderr tails, one-process and 2 GiB limits, timeout, owned termination, and unrelated-process survival. Cancellation tests cover before work, after qpdf starts, destination-partial copy, and the atomic commit boundary. Recovery covers every relevant nonterminal state and neighboring-file preservation. Leakage checks inspect SQLite, progress, errors, and diagnostics.

React tests cover add/drop, exact ordering, pointer and keyboard reorder, remove/focus behavior, validation, progress, cancellation boundary, success/failure/interrupted states, and axe. A real Tauri window is also visually inspected at 1280×800 using the production dependency diagnostic.

## G03 regression matrix

Rust session tests cover invalid/expired IDs, zero/out-of-bounds/over-1-MiB ranges, repeat/overlap, four-read concurrency, source replacement/change, reparse and hard-link behavior, write/delete sharing denial, two-session limit, explicit close/shutdown and no path leakage. Migration tests cover fresh/idempotent/checksum behavior, schema registration at 4, exact UTF-8/SQL byte limits, canonical SHA-256, metadata-only columns and backward-compatible jobs without plans.

Plan/unit tests cover each payload, 0-based bounds, duplicate rejection, exact permutation, removal complement, rotation degrees, output naming, full split partitions and 128-output limit. Real qpdf integration uses deterministic adversarial page markers to prove five operation semantics and source non-mutation. It separately covers fresh page-count mismatch, encrypted rejection, pre-execution cancellation, structure/count/hash/rotation verification, collision/failure cleanup, every expected split row, truthful partial publication and restart recovery.

Vitest covers raw range normalization without base64, PDF.js 256 KiB/four-read transport, repeat/overlap, bounded cancellable Unicode search, image-only behavior, component keyboard/focus/state flow and existing G01/G02 regressions. It does not claim rendering evidence.

Playwright runs real Chromium with the actual PDF.js worker, canvas, text layer, Resize/Intersection behavior and a test transport implementing the same range contract. Deterministic 100- and 1,000-page mixed-size PDFs prove first-page priority, virtualized mounts, off-screen cancellation/release, zoom/fit, last-page navigation, incremental search, no external popup/navigation, no action execution, and ephemeral organizer state until Apply.

A Windows-only WebView2 smoke launches the `test-runtime` binary, connects with test-only CDP and invokes real Tauri IPC. It proves raw 256 KiB PDF bytes, sanitized metadata, five retained-handle open/read/close samples and stale-generation rejection. Test fixture IPC and remote-debugging flags are absent from production command registration/startup. A native-window inspection additionally proves the backend Ctrl+O dialog, real 44-page PDF.js render, thumbnails, selectable text, sanitized filename display and explicit Close. Physical Explorer-to-app drop, forced-colors, 200% application zoom and reduced-motion checks remain explicit manual pre-PR evidence because the available GUI driver cannot drag across window boundaries or change host accessibility settings. jsdom alone is never accepted as viewer evidence.
