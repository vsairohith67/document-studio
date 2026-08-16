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
