# G03 — Viewer and Core PDF

Outcome: deliver the real local Document Studio PDF workspace on top of the accepted G01/G02 architecture.

Implement one-PDF open and Rust-side Tauri drop, opaque retained-handle viewer sessions, raw bounded range IPC, locally packaged PDF.js 6.2.108, progressive/virtualized pages and thumbnails, zoom/fit/navigation, temporary view rotation, text selection and bounded incremental search. Viewer state is ephemeral and must release all PDF.js/session resources on close.

Implement the accessible page organizer and durable `1.0.0` operations `pdf.extract-pages`, `pdf.remove-pages`, `pdf.reorder-pages`, `pdf.rotate-pages` and `pdf.split`. Persist a canonical hashed 64 KiB page-plan envelope through migration 4, create every expected output row before processing, reuse only bundled qpdf 12.3.2 in the accepted AppContainer/Job Object, verify every staging result independently, and publish through G01 without overwrite. Split completion requires every output; partial publication is a truthful failed state and preserves published files.

Security constraints: no raw path in the viewer webview, no custom protocol or COM drop, no PDF scripting/forms/attachments/external navigation, no CDN/runtime fetch, no shell/HTTP/general filesystem capability, no password persistence or qpdf password transport, no text/thumbnails/document bodies in SQLite or logs, and no removal of accepted G01/G02 APIs.

Evidence: TypeScript/Vitest, Rust migration/session/operation/recovery/security tests, real Chromium PDF.js worker/canvas/text/range tests, real raw Tauri IPC WebView2 smoke, deterministic adversarial qpdf fixtures, measured 100/1,000-page performance, docs/ADRs/CI, and a clean final pre-commit audit. Stop before staging, commit, push, PR, merge, release or G04.
