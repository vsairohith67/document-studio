# Platform Strategy

## Decision

Build the desktop product first. It gives the shortest path to speed and privacy because files stay on the machine and mature native command-line engines can be used directly. Build a web version only after the operation contract and verification model are stable.

## Shared product layers

- Shared React component library and design tokens.
- Shared operation manifests and JSON Schemas.
- Shared naming, validation, warning and error language.
- Shared test fixtures and acceptance cases.

## Platform-specific execution

### Desktop

- Tauri 2 shell and Rust orchestration.
- Direct adapters to signed/bundled qpdf, libvips and selected utilities.
- Optional separately installed LibreOffice, OCRmyPDF and language packs.
- SQLite history/presets/workflows.

### Web

- Browser-local path for operations that can be safely implemented with PDF.js/pdf-lib/WASM and fit browser memory.
- Cloud worker path for Office conversion, heavy OCR, AI and very large files.
- Clear local/cloud badge before execution.

### Mobile

- Capture, crop, dewarp, rotate, annotate, sign, simple organize/compress.
- Share-sheet integration and optional handoff to desktop/web.
- Heavy Office/OCR/AI features can be deferred or delegated with consent.

## Why not build every platform at once

Simultaneous desktop, web and mobile builds multiply packaging, testing, security and performance variables before the core operation lifecycle is proven. One strong vertical slice is more valuable than three incomplete shells.
