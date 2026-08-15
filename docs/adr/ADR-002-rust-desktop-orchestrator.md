# ADR-002: Use the Tauri Rust core as the desktop orchestrator

Status: Accepted

## Decision

Do not introduce a separate Go sidecar for the desktop foundation. Use Rust for job lifecycle, paths, IPC, cancellation and adapter execution. Reserve Go for a future cloud control plane.

## Rationale

This removes one local runtime and packaging boundary while retaining performance and strong Tauri integration.

## Revisit when

A desktop workload requires an independently scalable process or the Rust implementation creates a proven delivery bottleneck.
