# ADR-013: Bounded Images-to-PDF Writer and Deferred PDF Raster Renderer

## Status

Writer dependency decision approved for G04B branch review on 22 August 2026. The G04B product slice itself remains unaccepted and unmerged. The separate PDF-to-images renderer decision is blocked and remains unimplemented.

## Context

G04B introduces two independently gated operations: `image.to-pdf@1.0.0` and `pdf.to-images@1.0.0`. Both must remain local, deterministic, cancellable and subject to the existing no-overwrite publication boundary. Adding an unreviewed native renderer, AGPL component, runtime download, shell lookup or system installer would violate the repository's dependency and privacy rules.

## Decision: images to PDF

Document Studio adopts these exact crates from crates.io:

| Crate | Version | crates.io SHA-256 | Licence | Narrow role |
|---|---:|---|---|---|
| `image` | 0.25.10 | `85ab80394333c02fe689eaf900ab500fbd0c2213da414687ebf995a65d5a6104` | MIT OR Apache-2.0 | Decode only JPEG, PNG and WebP; inspect dimensions and orientation |
| `pdf-writer` | 0.15.0 | `f5e456864a7a304047bff84977dc6fb162bd956475d40ba50b2dcecaada7f753` | MIT OR Apache-2.0 | Original in-process PDF object writer |
| `flate2` | 1.1.9 | `843fba2746e448b37e26a819579957415c8cef339bf08564fe8b7ddbd959573c` | MIT OR Apache-2.0 | Deterministic zlib streams through the pure-Rust backend |

`image` uses `default-features = false` with only `jpeg`, `png` and `webp`. `pdf-writer` also disables default features. `flate2` selects `rust_backend`, so this slice adds no DLL, executable, installer, PATH lookup, runtime download, network call, shell permission or operating-system service.

The exact resolved subtree is committed in `Cargo.lock`. The dependency review found no exact-version GitHub Advisory matches for the direct crates or their resolved codec/compression subtree on the decision date. Release review must repeat the licence, checksum and advisory checks; this point-in-time result is not a permanent safety claim.

The writer accepts 1-128 selected files, identifies JPEG/PNG/WebP from content, preserves the selected order and writes one image per page. It rejects an input axis above 8,192 pixels, an image above 16,777,216 pixels, a selected set above 67,108,864 decoded pixels or 512 MiB of source bytes. Decoder allocation is capped. EXIF orientation is applied once; alpha is emitted through a soft mask; ICC profiles are not retained, decoded pixel values use DeviceRGB and ICC-bearing inputs produce a sanitized warning. The operation does not add OCR, optimization, metadata editing, encryption or recursion.

The existing qpdf 12.3.2 bundle independently checks PDF structure, encryption state and page count. The service then reopens the staged output, hashes it, proves every source identity/size/hash is unchanged and publishes through the existing durable no-overwrite protocol. Failure or cancellation removes owned staging state and never reports an unverified output as complete.

## Decision: PDF to images

`pdf.to-images@1.0.0` is dependency-blocked. No production implementation, dormant runtime path or UI promise is authorized.

- [PDFium](https://pdfium.googlesource.com/pdfium/) publishes source and Chromium-style build instructions, but the review did not identify an official, signed, versioned Windows runtime artifact with an acceptable redistribution/provenance chain.
- [MuPDF's official releases page](https://mupdf.com/releases) states that embedded product use requires AGPL compliance or a commercial licence; neither route is authorized for this goal.
- libvips does not itself establish an acceptable PDF rendering provenance chain. Its PDF loading still depends on an independently reviewed renderer/native package.
- The accepted PDF.js package is a reduced, local viewer renderer. Reusing it for durable raster export would create a new worker-to-publication architecture and requires its own approved design and verification evidence.

The Convert UI truthfully displays the dependency blocker and disables the PDF-to-images direction. It must not download a renderer, search the machine for one, invoke a shell, or silently use another engine.

## Alternatives considered

- Embed source images without decoding: rejected because mixed formats, orientation, alpha, ICC handling and content validation would not have one bounded deterministic path.
- Use a general image conversion executable: rejected because it increases native distribution and process-sandbox surface for the narrow writer operation.
- Adopt an unofficial PDFium Windows repack: rejected because third-party binary provenance does not satisfy the production dependency gate.
- Adopt MuPDF under AGPL: rejected because the requested product/distribution obligations are not approved.
- Use PDF.js immediately for export: deferred pending a separate architecture decision covering an opaque renderer session, bounded page raster transport, cancellation and durable multi-output publication.

## Consequences

G04B can deliver and independently accept images-to-PDF while accurately recording PDF-to-images as blocked. The capability is not removed from the roadmap. Accepting a renderer later requires a superseding or companion ADR with an exact version/artifact, checksum/signature chain, notices, sandbox, limits, failure behavior and output-verification plan.
