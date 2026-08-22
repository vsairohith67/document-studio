# G04 — Optimize and Convert

G04 is delivered as bounded, independently accepted slices. A blocked dependency must be recorded truthfully; it does not remove an approved capability from the roadmap.

## Slice status

- **G04A — COMPLETE:** `pdf.compress-lossless@1.0.0`, accepted on main merge `a27306653119e6e4fcdef162308445b78129f974`.
- **G04B — active implementation:** `image.to-pdf@1.0.0` is implemented for review on `feat/g04b-image-pdf-convert`; G04B is not accepted or complete. `pdf.to-images@1.0.0` is independently dependency-blocked and has no production renderer path.
- **G04C — later:** balanced compression and bounded image-quality controls.
- **G04D — later:** isolated Office-to-PDF conversion.
- **G04E — later:** text, Markdown and local-HTML-to-PDF conversion.
- **G04F — later:** deterministic isolated batch execution.

## G04B contract

Images-to-PDF accepts 1-128 content-sniffed JPEG/JPG, PNG or WebP inputs in exact selected order and produces one independently verified, unencrypted PDF with one image per page. It applies fixed dimension, decoded-pixel, selected-total and source-byte limits; applies EXIF orientation once; handles alpha and ICC deterministically; preserves source files; and uses the accepted cancellable, recoverable, no-overwrite publication boundary. It does not imply OCR, optimization, encryption, metadata editing or recursive ingestion.

PDF-to-images remains in scope with output format JPEG/PNG/WebP, DPI choices 72/150/300, at most 128 pages, 8,192-pixel axes and 16,777,216 pixels per output. Production work cannot begin until an exact renderer version/artifact passes provenance, licence, redistribution, sandbox, cancellation and output-verification review. No runtime download, shell/system lookup or unreviewed fallback is allowed.

The Convert UI must expose both directions honestly, keep the blocked direction disabled with a user-safe reason, and provide full keyboard/accessibility support. G04B database additions remain metadata-only and may not alter G03 page-plan semantics.
