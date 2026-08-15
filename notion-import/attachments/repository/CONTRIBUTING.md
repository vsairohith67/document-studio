# Contributing to Document Studio

Document Studio is built in small, verified vertical slices. A change is complete only when the user-visible behavior, typed contract, processing adapter, verification, cleanup, tests and documentation move together.

## Development flow

1. Start from a short-lived branch named `feature/<scope>`, `fix/<scope>` or `docs/<scope>`.
2. Read `AGENTS.md`, the relevant architecture document and any accepted ADR before editing.
3. Keep the change to one coherent outcome; do not combine unrelated refactors.
4. Add or update tests before requesting review.
5. Run the repository validator, link checker, frontend typecheck/build, Rust tests and relevant adapter tests.
6. Include screenshots for UI changes and benchmark evidence for performance-sensitive changes.
7. Update documentation and add an ADR when a durable technical decision changes.

## Pull-request completion gate

- Original inputs remain unchanged by default.
- Output is staged, reopened and verified before publication.
- Cancellation, cleanup and actionable error handling are covered.
- Secrets and document contents are absent from logs and diagnostics.
- Accessibility works by keyboard, at 200% zoom and with visible focus.
- New dependencies include purpose, version/provenance, license and update policy.

See `docs/16-TEST-AND-QUALITY-STRATEGY.md` and `docs/22-IMPLEMENTATION-READINESS-CHECKLIST.md`.
