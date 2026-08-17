# Architecture Decision Records

| ADR | Decision | Status |
|---|---|---|
| [ADR-001](ADR-001-desktop-first.md) | Ship Windows desktop first | Accepted |
| [ADR-002](ADR-002-rust-desktop-orchestrator.md) | Use the Tauri Rust core as desktop orchestrator | Accepted |
| [ADR-003](ADR-003-local-first-ai-optional.md) | Keep AI optional and provider-neutral | Accepted |
| [ADR-004](ADR-004-output-verification.md) | Require output verification before publication | Accepted |
| [ADR-005](ADR-005-storage-ownership-and-provider-persistence.md) | Keep documents user-owned and provider persistence deferred | Accepted |
| [ADR-006](ADR-006-foundation-dependencies-and-sqlite.md) | Adopt reviewed foundation dependencies and bundled SQLite | Accepted |
| [ADR-007](ADR-007-durable-publication-and-recovery.md) | Require journaled no-overwrite publication and evidence-based recovery | Accepted |
| [ADR-008](ADR-008-single-instance-cancellation.md) | Enforce one Windows application process for cancellation ownership | Accepted |
| [ADR-009](ADR-009-qpdf-and-production-pdf-merge.md) | Bundle qpdf for sandboxed, verified, page-only PDF Merge | Accepted; implemented in G02 |

Create a new ADR for decisions that are expensive to reverse, affect security/privacy, change the operation contract, add a production dependency/model or alter the platform sequence. Never rewrite an accepted ADR to hide history; supersede it with a new record.
