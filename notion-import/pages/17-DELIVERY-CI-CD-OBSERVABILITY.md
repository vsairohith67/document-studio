# Delivery, CI/CD and Observability

## Branch and release flow

- Protected `main`.
- Short-lived feature branches.
- Pull request requires specification validation, formatting, typecheck, tests and security/dependency review for new engines.
- Signed tags and release artifacts.
- Reproducible sidecar acquisition with checksums.

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

## Future cloud observability

- Metrics: queue wait, processing time by stage, failure code, worker saturation, upload/download duration and cleanup success.
- Distributed tracing without document content.
- Security audit logs and deletion proofs.
- SLOs defined only after real load tests.
