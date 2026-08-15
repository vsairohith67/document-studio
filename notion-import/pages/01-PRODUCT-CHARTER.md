# Product Charter

## Product statement

Document Studio is a fast, beautiful and private document workspace for converting, organizing, optimizing, editing, OCR, forms, signatures, security, automation and optional document intelligence. It should feel like one coherent application rather than dozens of disconnected upload pages.

## Primary outcome

A user can drop one or many files, choose an operation, inspect the exact pages/settings, run a cancellable job, verify the result and save a new file without losing the original.

## Core principles

1. **Local first.** Routine PDF and image operations require no upload.
2. **Truthful completion.** Success appears only after output verification.
3. **Speed by architecture.** Avoid upload latency, warm heavy engines, stream progress, cache previews and choose the fastest safe execution path.
4. **One complete edition.** No artificial feature partitions in the personal build.
5. **Preview before irreversible work.** Redaction, signatures, destructive page changes and security settings require an explicit review.
6. **Original product design.** Match useful capabilities and performance expectations, not another product's brand, copy, assets or source code.
7. **Progressive power.** Simple defaults first; advanced controls are available without overwhelming routine users.
8. **Accessible by default.** Keyboard, screen reader, high contrast, reduced motion and zoom are design constraints.

## Platform decision

| Order | Platform | Why |
|---|---|---|
| 1 | Windows desktop | Fastest local processing, simplest privacy story, highest value for school/office workflows |
| 2 | macOS desktop | Reuse the Tauri/React workbench with platform packaging and Apple Silicon model options |
| 3 | Optional web app | Reach and shareability; browser-local fast path plus cloud workers for unsupported/heavy tasks |
| 4 | Android/iOS companion | Scan, capture, annotate, sign, send to desktop/web; do not duplicate every heavy converter initially |

## Success measures

- First meaningful result in under 60 seconds for a new user.
- Routine merge/split/rotate jobs complete locally without network access.
- No false-success reports in automated recovery tests.
- 95% of core workflows operable by keyboard.
- Performance budgets in `08-PERFORMANCE-ARCHITECTURE.md` met on the reference machine.
