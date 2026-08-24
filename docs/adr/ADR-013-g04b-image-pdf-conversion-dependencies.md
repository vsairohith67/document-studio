# ADR-013: Bounded Image/PDF Conversion Engines

## Status

Superseded in part on 22 August 2026. The images-to-PDF dependency decision is accepted on main at `6940a1381822a5872f4c345cc0e5cd15b2e6294c`. The former PDF-raster renderer blocker is replaced by the G04B2 decision below, approved for implementation on `feat/g04b2-pdf-to-images`. G04B2 remains unaccepted until its owner merge gate.

## Context

G04B defines two independently versioned local operations: `image.to-pdf@1.0.0` and `pdf.to-images@1.0.0`. Both must be bounded, cancellable, verifiable and subject to the durable no-overwrite publication boundary. Runtime downloads, CDN access, system renderer discovery and an unreviewed native renderer would violate the dependency and privacy rules.

## Accepted decision: images to PDF

Document Studio uses these exact crates from crates.io:

| Crate | Version | crates.io SHA-256 | Licence | Narrow role |
|---|---:|---|---|---|
| `image` | 0.25.10 | `85ab80394333c02fe689eaf900ab500fbd0c2213da414687ebf995a65d5a6104` | MIT OR Apache-2.0 | Decode JPEG, PNG and WebP; encode G04B2 JPEG, PNG and lossless WebP |
| `pdf-writer` | 0.15.0 | `f5e456864a7a304047bff84977dc6fb162bd956475d40ba50b2dcecaada7f753` | MIT OR Apache-2.0 | Original in-process PDF object writer |
| `flate2` | 1.1.9 | `843fba2746e448b37e26a819579957415c8cef339bf08564fe8b7ddbd959573c` | MIT OR Apache-2.0 | Deterministic zlib streams through the pure-Rust backend |

`image` uses `default-features = false` with only `jpeg`, `png` and `webp`. `pdf-writer` disables default features. `flate2` selects `rust_backend`. The resolved subtree is pinned in `Cargo.lock`; release review must repeat point-in-time licence, checksum and advisory checks.

The accepted writer takes 1-128 content-sniffed images, applies the documented orientation, alpha and colour rules, enforces 8,192-pixel axes, 16,777,216 pixels per image, 67,108,864 aggregate decoded pixels and 512 MiB aggregate source bytes, then verifies with bundled qpdf 12.3.2 before durable publication.

## G04B2 decision: PDF to images

The already accepted local `pdfjs-dist` 6.2.108 package is the approved production renderer for `pdf.to-images@1.0.0`. G04B2 introduces no PDFium, MuPDF, Poppler, PDFBox, native renderer, executable, DLL, installer, runtime download, CDN request or system renderer lookup.

Source bytes remain owned by opaque Rust viewer/job sessions. React receives a session ID, generation, safe display metadata, page plans and destination grant IDs; source and destination paths remain outside React. PDF.js reads bounded byte ranges through the existing raw Tauri transport and local matching worker/assets. It renders exactly one selected page at a time into a private, unattached canvas at `scale = DPI / 72`, print intent and an explicit opaque-white background.

After each render, the page's raw RGBA bytes return through binary Tauri IPC. The request carries an authenticated job ID, render-session ID, strict page ordinal, one-use nonce and expected dimensions in headers. Rust checks ownership, sequence, nonce, cancellation, dimensions, caps and exact `width * height * 4` body length before consuming the nonce. JSON integer arrays, base64 and destination paths are prohibited. Browser `toBlob()` and `convertToBlob()` are not durable encoders.

Rust flattens the required opaque pixels to RGB and uses the existing `image` 0.25.10 only:

- PNG is lossless and records deterministic pixels-per-metre density metadata derived from 72, 150 or 300 DPI.
- JPEG uses fixed quality 92, opaque white and JFIF DPI metadata. No public quality control exists in v1.
- WebP is lossless only. Lossy WebP requires a separate encoder and public-contract decision.

The public operation accepts exactly one unencrypted local PDF, 1-128 unique pages in the selected output order, JPEG/PNG/lossless WebP and exactly 72, 150 or 300 DPI. Every page must be finite and no larger than 8,192 by 8,192 or 16,777,216 pixels. The complete plan is evaluated before rendering and may not exceed 67,108,864 estimated pixels. This reuses the accepted conservative four-maximum-page work budget; it prevents a nominal 128-page limit from becoming an unbounded job.

The renderer keeps the accepted local security settings: `isEvalSupported: false`, XFA disabled, form rendering disabled and annotations disabled. G04B2 creates no scripting manager, annotation/form layer, attachment UI or external-link UI. The CSP has no runtime document network path. The diagnostics record the actual WebView2 runtime version as evidence; GPU acceleration may be used naturally but is not required.

The browser/PDF.js canvas result defines the output colour contract. G04B2 does not claim source colour-space or profile preservation and does not promise byte-identical pixels across WebView2, Chromium or Skia versions. PNG and lossless WebP are checked exactly against the received canvas pixels; JPEG uses a documented mean absolute pixel-error bound. Each staged file is magic-checked, decoded, dimension-checked, hashed and matched to its requested format/page before publication.

Each page follows `render -> extract RGBA -> authenticated transfer -> encode -> verify -> release -> next page`; all canvases and raw buffers are not retained. Cancellation aborts range work, calls `RenderTask.cancel()`, stops new pages and is checked between render, transfer, encode, verify and publication phases. `getImageData()` is synchronous, so interruption is checked immediately before and after its bounded call rather than claimed to be instantaneous.

All output rows and deterministic names are reserved before rendering. Staging membership must exactly equal the plan before publication. User-visible multi-file publication is not filesystem-atomic; truthful partial-publication evidence preserves already published user files and cleans only owned unpublished staging.

## Alternatives considered

- Add PDFium, MuPDF, Poppler or PDFBox: rejected because no new native or Java renderer is needed or authorized.
- Use a third-party renderer executable or system installation: rejected because provenance, distribution, shell and discovery surfaces exceed the local boundary.
- Encode with browser blobs: rejected because browser codec behavior and durable file publication are not the accepted Rust verification boundary.
- Transfer RGBA in JSON/base64: rejected because it expands memory, serialization and confusion/replay risk.
- Allow arbitrary DPI or lossy WebP: rejected because v1 requires a small auditable contract and the accepted encoder exposes lossless WebP only.

## Consequences

G04B2 extends the accepted PDF.js viewer renderer without adding a dependency or capability. Acceptance requires native Windows WebView2 evidence, focused renderer fixtures, raw-handler security inspection, exact-head CI and an owner-controlled merge. JBIG2/JPX evidence is required when a repository-owned fixture is available; no such fixture is currently present, so no support claim beyond PDF.js 6.2.108 behavior is made.
