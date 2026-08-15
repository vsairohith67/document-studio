# System Architecture

## Desktop architecture

The desktop app uses Tauri as the trusted boundary. React renders the workbench. The UI invokes allow-listed typed commands. Rust validates paths/settings, creates a private job workspace, selects an operation adapter, streams structured progress, supports cancellation, verifies outputs and records history.

## Components

- **React workbench:** presentation, local state, validation display, thumbnails and editor scene graph.
- **Tauri command layer:** secure IPC and capabilities.
- **Rust job engine:** state machine, queue, resource scheduler, cancellation, recovery, audit.
- **Operation registry:** manifests, schemas, dependencies and adapter factories.
- **Adapters:** qpdf, libvips, LibreOffice, OCRmyPDF/Tesseract, optional Docling/model services.
- **SQLite repository:** settings, jobs, presets, workflows, dependencies and model state.
- **Workspace manager:** per-job directories, staging, atomic publication and cleanup.

## Future web architecture

- Static React web app/CDN.
- API gateway and Go control plane.
- PostgreSQL metadata, Redis/Valkey queue coordination and object storage.
- Isolated worker pools by engine/risk profile.
- Regional routing and explicit retention policy.
- Browser-local execution where safe and fast.

## Architectural boundary

The local desktop should not depend on the future Go service. Both implement the same operation contract and schemas, but desktop remains fully useful offline.
