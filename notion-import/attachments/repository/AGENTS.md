# AGENTS.md - Document Studio

## Repository intent

This repository contains the canonical plan and starter scaffold for Document Studio. Treat `docs/`, `packages/contracts/`, `packages/tokens/`, and `models/models.yaml` as specifications, not optional notes.

## Working method

- Inspect before editing.
- Make small vertical slices.
- Prefer tests before broad refactors.
- Keep desktop processing local by default.
- Never weaken verification, redaction, privacy or cancellation behavior to make a demo pass.
- Do not add an online dependency to a local operation without an architecture decision record.
- Do not import code from competitor products. Use public specifications, standards and original implementation.

## Required checks

Before marking work complete:

```bash
python scripts/validate_repo.py
python scripts/check_links.py
cargo fmt --all -- --check
cargo test --workspace
npm run typecheck
npm test
```

Only run commands that are applicable to the slice and installed environment, but report all skipped checks explicitly.

## UI rule

Use the design tokens in `packages/tokens/document-studio.tokens.json`. The visual direction is **Precision Paper**: calm, professional, document-first, high-density without clutter, one primary accent, no gratuitous gradients or glass effects, and full keyboard/accessibility support.
