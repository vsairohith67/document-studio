# Delivery, CI/CD and Observability

## Branch and release flow

- Protected `main`.
- Short-lived feature branches.
- Pull request requires specification validation, formatting, typecheck, tests and security/dependency review for new engines.
- Signed tags and release artifacts.
- Reproducible sidecar acquisition with checksums.

CI pins checkout, Python, Node, Rust toolchain and Cosign installer actions to immutable commit SHAs. Python installs with required hashes, npm uses `npm ci`, Cargo uses `--locked`, clippy denies warnings, and Windows compiles Tauri with `--no-bundle`. CI builds the isolated `test-runtime` feature and proves the single-instance boundary. For G02 it also acquires the signed qpdf archive into runner temporary storage, verifies provenance and every reviewed hash, compares all files byte-for-byte with the committed resource, then runs the real AppContainer/Job Object boundary test. The production Tauri build excludes the test probe. A dependency update must change its manifest and repeat licence/provenance review.

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

Progress events and SQLite history use allow-listed fields rather than raw logs. Errors expose a typed safe code, title and detail. Dependency diagnostics never install or download an engine at runtime. The implementation log records commands, exits and limitations without copying document contents or secrets.

## Future cloud observability

- Metrics: queue wait, processing time by stage, failure code, worker saturation, upload/download duration and cleanup success.
- Distributed tracing without document content.
- Security audit logs and deletion proofs.
- SLOs defined only after real load tests.
