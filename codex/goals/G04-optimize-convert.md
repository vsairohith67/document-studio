# G04 — Optimize and Convert

G04 is delivered as bounded, independently accepted slices. A blocked dependency must be recorded truthfully; it does not remove an approved capability from the roadmap.

## Slice status

- **G04A — COMPLETE:** `pdf.compress-lossless@1.0.0`, accepted on main merge `a27306653119e6e4fcdef162308445b78129f974`.
- **G04B — COMPLETE:** `image.to-pdf@1.0.0`, accepted on main at `6940a1381822a5872f4c345cc0e5cd15b2e6294c`.
- **G04B2 — active implementation:** `pdf.to-images@1.0.0` on `feat/g04b2-pdf-to-images`; G04B2 is not accepted or complete until its owner merge gate.
- **G04C — later:** balanced compression and bounded image-quality controls.
- **G04D — later:** isolated Office-to-PDF conversion.
- **G04E — later:** text, Markdown and local-HTML-to-PDF conversion.
- **G04F — later:** deterministic isolated batch execution.

## G04B contract

Images-to-PDF accepts 1-128 content-sniffed JPEG/JPG, PNG or WebP inputs in exact selected order and produces one independently verified, unencrypted PDF with one image per page. It applies fixed dimension, decoded-pixel, selected-total and source-byte limits; applies EXIF orientation once; handles alpha and ICC deterministically; preserves source files; and uses the accepted cancellable, recoverable, no-overwrite publication boundary. It does not imply OCR, optimization, encryption, metadata editing or recursive ingestion.

PDF-to-images uses accepted PDF.js 6.2.108 for sequential private-canvas rendering and existing `image` 0.25.10 for Rust-side encoding. It supports JPEG/PNG/lossless WebP, exactly 72/150/300 DPI, at most 128 ordered pages, 8,192-pixel axes, 16,777,216 pixels per output and a 67,108,864-pixel aggregate work budget. Authenticated raw binary IPC binds each one-use page transfer to its job/session/ordinal/nonce and expected dimensions. No runtime download, shell/system lookup, browser blob encoding or unreviewed renderer fallback is allowed.

The Convert UI exposes both directions honestly and provides full keyboard/accessibility support. PDF-to-images must reuse the G03 viewer/thumbnail infrastructure and keep source/destination paths outside React. G04B database additions remain metadata-only and may not alter G03 page-plan semantics.
