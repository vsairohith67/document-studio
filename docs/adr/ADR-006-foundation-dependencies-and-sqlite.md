# ADR-006: Foundation dependencies and SQLite implementation

Status: Accepted

## Decision

G01 uses a synchronous `rusqlite` repository on a dedicated blocking worker. SQLite is compiled through the `bundled` feature, runs with foreign keys enabled, WAL journaling, `synchronous=FULL`, and a bounded busy timeout. Migrations are ordered, checksummed, append-only, and transactional; startup fails closed if a migration cannot be trusted or applied.

Direct npm, Cargo, and Python dependencies are limited to the reviewed G01 dependency register. Manifests use narrow compatible version families where practical, while committed npm, Cargo, and hash-pinned Python lockfiles record the exact resolution and registry integrity evidence. Lock generation and dependency installation remain explicit reviewed implementation steps.

The Tauri dialog plugin is the only new frontend-facing native plugin. It is restricted to native open dialogs. The webview receives no shell capability and no general filesystem capability. G01 defers PDF.js and all production document engines because its reference operation is `diagnostic.copy`.

Standard SQLite is protected by the current user's application-data access controls. SQLCipher is not adopted in G01; it would add native packaging, cryptographic key-management, and licence review obligations without protecting document contents, which are prohibited from the database.

## Rationale

`rusqlite` gives the Rust core a small, direct embedded repository boundary without adding an asynchronous runtime or a second local service. Bundled SQLite makes the tested runtime version reproducible on Windows. Exact resolved lockfiles constrain supply-chain drift while allowing intentional upgrades through review.

## Consequences

- Database work must never run on the webview thread or block Tauri event handling.
- Application startup completes migrations and recovery before accepting job commands.
- Dependency diagnostics report installed/built-in/deferred state but do not download engines.
- PDF.js, qpdf, libvips, OCR, Office, cloud, and AI dependencies require later goal-specific adoption gates.
