# Delivery, CI/CD and Observability

## Branch and release flow

- Protected `main`.
- Short-lived feature branches.
- Pull request requires specification validation, formatting, typecheck, tests and security/dependency review for new engines.
- Signed tags and release artifacts.
- Reproducible sidecar acquisition with checksums.

G01 CI pins checkout, Python, Node and Rust toolchain actions to immutable commit SHAs. Python installs with required hashes, npm uses `npm ci`, Cargo uses `--locked`, clippy denies warnings, and Windows compiles Tauri with `--no-bundle`. CI also builds the isolated `test-runtime` feature and launches two processes to prove the secondary cannot reach setup. The production Tauri build excludes that feature. A dependency update must change its manifest and lock together and repeat licence/provenance review.

## Desktop packaging

- Windows MSI/NSIS first.
- macOS universal/architecture-specific package later.
- Optional engine bundles separated when licensing or size requires it.
- Application update channel is signed and can be disabled.

## Local observability

- Structured logs with correlation/job ID.
- Stage timings and engine version.
- No document text, password, key or prompt body by default.
- User-controlled diagnostics export with redaction.
- Local benchmark history to detect regressions.

G01 progress events and SQLite history use allow-listed fields rather than raw logs. Errors expose a typed safe code, title and detail. Dependency diagnostics report built-in or deferred state only and never install or download an engine. The implementation log records commands, exits and limitations without copying document contents or secrets.

## Future cloud observability

- Metrics: queue wait, processing time by stage, failure code, worker saturation, upload/download duration and cleanup success.
- Distributed tracing without document content.
- Security audit logs and deletion proofs.
- SLOs defined only after real load tests.
