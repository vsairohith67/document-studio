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

## G02 production path

`pdf.merge` is dispatched through the operation registry while `diagnostic.copy` remains a regression operation. Rust freezes one ASCII-named workspace snapshot per persisted ordinal. Expensive validation is shared only by source file identity and runs with bounded concurrency; snapshots and qpdf `--file` arguments are never deduplicated.

The verified bundled qpdf 12.3.2 executable runs through direct `CreateProcessW` arguments in the fixed zero-capability `DocumentStudio.PdfEngine.Qpdf.V1` AppContainer. A private Job Object enforces kill-on-close, one active process, a 2 GiB process-memory limit, and owned termination. There is no shell, network capability, PATH discovery, random profile, or unsandboxed fallback.

Output stays in the owned workspace until Rust file checks, a new SHA-256/size read, strict qpdf reopen, encryption check, and exact page-count sum pass. G01 then performs destination-local no-overwrite publication and final hash/size equality. Restart recovery never resumes a merge.

## G03 viewer and organizer path

The Viewer is a second workbench mode; it does not replace PDF Merge or any IPC v1 command. A backend native dialog or Rust-side Tauri drop opens one validated PDF through a retained read-only Windows handle that denies write/delete sharing. An opaque session/generation maps to that handle and identity record. PDF.js receives only raw bounded range responses and same-origin packaged assets; neither PDF.js nor React receives the source path.

React lazy-loads the viewer chunk, PDF.js legacy ESM API and matching worker. TanStack Virtual mounts visible pages/thumbnails plus small overscan. Page metadata is reused while render tasks, canvases, text layers and indexing are cancellable and bounded. Close destroys the loading task/document and releases the Rust session.

Apply/Export converts ephemeral organizer state into one durable operation-plan envelope. Rust pins the viewer handle, writes one ASCII snapshot, rechecks qpdf page count, runs one of five registered operations through the accepted AppContainer/Job Object, independently verifies staging outputs, and calls G01 publication. Split inserts every output record before execution and never claims cross-file atomicity.
