# G04B Image/PDF Conversion Implementation Log

## Scope and split

G04B originally gated the two approved operations independently:

- `image.to-pdf@1.0.0`: accepted on main at `6940a1381822a5872f4c345cc0e5cd15b2e6294c`.
- `pdf.to-images@1.0.0`: implemented separately by G04B2 with the accepted PDF.js 6.2.108 renderer; see [the G04B2 log](G04B2-pdf-to-images.md).

G04C balanced compression, G04D Office conversion, G04E text/Markdown/local-HTML conversion and G04F batch execution remain later slices. No G04C-G04F production implementation is part of this branch.

## Dependency gate

[ADR-013](../adr/ADR-013-g04b-image-pdf-conversion-dependencies.md) records the exact `image` 0.25.10, `pdf-writer` 0.15.0 and `flate2` 1.1.9 crates, hashes, licences and narrow features. The writer is in-process and adds no runtime network, installer, system lookup, shell command or native binary. The ADR now also records G04B2's approved reuse of the already accepted PDF.js renderer without adding a native renderer.

## Durable contract and database

Migration 5 adds two metadata-only tables without changing accepted G03 page plans:

- `job_operation_specs` binds a job to one operation/schema version, canonical settings JSON and SHA-256; SQLite rejects invalid JSON and settings over 65,536 UTF-8 bytes.
- `job_warnings` stores only sanitized code/detail plus optional input/page indices. It has no document byte, extracted text, credential or raw-path column.

The operation fixes content formats, ordering and resource limits in its public contract. Each source is privately snapshotted and rechecked by Windows identity, size and SHA-256 before publication.

## Images-to-PDF implementation

- Accepts 1-128 content-sniffed JPEG/JPG, PNG or WebP files in the exact displayed order.
- Rejects malformed content, unsupported magic, dimensions above 8,192 per axis, more than 16,777,216 pixels per input, more than 67,108,864 selected decoded pixels and more than 512 MiB selected source bytes.
- Applies EXIF orientation once, preserves alpha with a PDF soft mask and records a safe warning when an ICC profile is not retained and decoded pixel values use DeviceRGB.
- Writes one image per PDF page with the oriented image dimensions as the page MediaBox and no margin.
- Uses lossless Flate image streams; no OCR, optimization, metadata editing, encryption or recursive folder ingestion is implied.
- Uses qpdf strict checking plus encryption/page-count checks, reopens and hashes the output, proves sources unchanged, then publishes with the accepted durable collision-safe protocol.
- Treats cancellation, malformed input, verification failure, source mutation and destination collision as non-success states and cleans only owned temporary data.

## Desktop UX

Convert has two explicit direction tabs. Images to PDF provides native file selection, drag/drop, visible ordering, keyboard reorder/remove controls, destination and output-name fields, progress, cancellation and result state. G04B2 enables PDF to images through the shared opaque viewer/thumbnail infrastructure without changing the accepted images-to-PDF flow.

## Verification inventory

The G04B suite covers content-over-extension detection, JPEG/PNG/WebP, alpha soft masks, EXIF orientation, ICC warnings, selected-order MediaBoxes, qpdf strict/page-count verification, source immutability, malformed and oversized input, the exact 128-input acceptance/129-input rejection boundary, early and final-page cancellation, collision safety, injected publication failure, startup recovery, altered persisted-settings rejection, database constraints and accessible keyboard UI. A browser-backed test renders a generated first page through the accepted PDF.js viewer and compares its sampled pixel with the source image. The `pretest:browser` lifecycle compiles and executes the exact native acceptance fixture producer before Playwright starts, verifies that its source and PDF evidence exist, and leaves Playwright's page-interaction timeout dedicated to the renderer comparison. Repository validation and the G04B boundary verifier pin that sequencing along with the dependency, schema, scope, visual-evidence and capability invariants.

G04B passed its owner merge gate and is accepted on main. G04B2 keeps its own exact-head CI, review and owner-controlled merge gate.
