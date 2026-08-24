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

React lazy-loads the viewer chunk, PDF.js legacy ESM API and matching worker. TanStack Virtual mounts viewport pages/thumbnails plus small overscan; measured scroll-container geometry marks actual visibility independently, so overscan cannot define the current page. Candidate loading retains separate ownership until page-one metadata validates, then swaps atomically. Render tasks, canvases, text layers and indexing are cancellable and bounded; Close disposes active and candidate PDF.js/Rust sessions.

Apply/Export converts ephemeral organizer state into one durable operation-plan envelope. Rust pins the viewer handle, writes one ASCII snapshot, rechecks qpdf page count, runs one of five registered operations through the accepted AppContainer/Job Object, independently verifies staging outputs, and calls G01 publication. Split inserts every output record before execution and never claims cross-file atomicity.

## G04A optimization and G04B conversion paths

G04A lossless compression is an accepted direct qpdf operation that reuses the G02 sandbox, verification and publication boundary without adding a codec or a Tauri capability.

G04B's `image.to-pdf@1.0.0` path is an in-process Rust adapter. It content-sniffs and privately snapshots 1-128 JPEG/PNG/WebP inputs, applies bounded decoding and orientation/alpha rules, emits one image per PDF page, then uses qpdf only as an independent verifier before durable publication. Migration 5 binds each job to canonical hashed settings in `job_operation_specs` and records only sanitized metadata in `job_warnings`; it does not alter G03 page plans or persist document bodies.

G04B2's `pdf.to-images@1.0.0` adapter reuses the accepted PDF.js 6.2.108 renderer and opaque G03 viewer sessions. React sees no source or destination path. It preflights the entire ordered page plan, then renders one private opaque-white canvas at a time and sends raw RGBA as an authenticated binary Tauri request. Rust enforces job/session/page/nonce ownership, dimensions, exact payload length, per-page caps, the aggregate work budget and cancellation before encoding PNG, fixed-quality JPEG or lossless WebP with existing `image` 0.25.10. Each output is decoded, dimension/format/pixel checked and hashed before the accepted durable multi-output publication loop. No browser blob encoder, new native renderer, runtime download, system discovery, app-defined custom protocol, network capability or Tauri capability is added.
