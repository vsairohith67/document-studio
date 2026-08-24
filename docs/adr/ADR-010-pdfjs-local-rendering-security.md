# ADR-010: Local PDF.js rendering and security

Status: Accepted; implemented in G03

## Context

G03 needs a fast, searchable PDF viewer without exposing source paths, running document code, using a CDN, or copying Mozilla's generic viewer UI. The accepted desktop stack is Node 24, Vite, TypeScript, React and WebView2.

## Decision

Use the official `pdfjs-dist` package at exactly `6.2.108`, published 28 July 2026 from Mozilla's PDF.js project under Apache-2.0. The npm lock records the official registry tarball, integrity `sha512-YxFb+SQcodN2rnX9Tn3dHYlqfb7NjlzzfONPpJd+AKoKtUjEdevTfbC07d5TcczzOK6261auRkP/M8OBHs9vFQ==`, 550 unpacked files and 34,497,725 unpacked bytes. The package requires Node `>=22.13.0 || >=24`, which matches the repository's Node 24 baseline.

Use the legacy ESM API/worker build for current WebView2 compatibility. This is still PDF.js 6.2.108; it is not an older package. Vite bundles only the display API used by Document Studio. The build script copies the matching legacy worker plus CMaps, standard fonts, ICC profiles and WASM into same-origin application assets. It checks the embedded worker version and emits a worker SHA-256 manifest. The staged set is 202 files and 5,892,091 bytes; `pdf.worker.mjs` SHA-256 is `b4e582882f5e811f4d1b7b511f68d9a0c3209141e6f68856f01408c5cc155131`.

No CDN or runtime fallback is allowed. Main API and worker versions must match. The worker, CMaps, fonts, ICC and WASM files are exact path/size/SHA-256 manifest entries staged through an atomic owned-directory replacement; generic viewer, QuickJS, fallback/debug/map/test assets are excluded. PDF.js receives bytes only through the opaque-session range transport. Explicit loading/configuration includes `isEvalSupported: false`, `enableXfa: false`, `renderForms: false`, `stopAtErrors: true`, bounded range loading and bounded image/canvas work. Page rendering uses `AnnotationMode.DISABLE`; Document Studio creates no interactive annotation/form layer, scripting manager, attachment UI or external-link UI. Password callbacks live only in component memory; passwords are not persisted, logged, placed in diagnostics or sent to qpdf.

The application CSP is:

```text
default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data: blob:; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none';
```

Rust blocks navigation and new windows outside the application origin. `ipc:` and `http://ipc.localhost` are Tauri's built-in authenticated command transport origins; G04B2 permits them narrowly so raw RGBA stays binary instead of falling back to JSON `postMessage`. They are not an app-defined custom protocol, remote-network route or HTTP capability. There is no `unsafe-eval`, runtime document network, custom application protocol or general filesystem permission.

The official Mozilla advisory GHSA-wgrm-67xf-hhpq/CVE-2024-4367 affected `pdfjs-dist <=4.1.392` and was fixed in 4.2.67; G03 keeps the documented defense-in-depth setting `isEvalSupported: false`. No separate 2026 advisory is claimed because no official Mozilla/GitHub advisory establishing one was retrieved.

## Alternatives rejected

- The modern build is smaller but was less compatible with the accepted WebView2/TypeScript build surface during implementation; the exact-version legacy build is reversible at a later dependency review.
- A React PDF viewer wrapper would add dependency and UI/security behavior outside Document Studio's control.
- Mozilla's generic viewer is not used because the product requires the Quiet Precision workbench, its own accessibility model and a deliberately smaller security surface.

## Consequences

PDF.js is a hostile-document parser inside the webview and remains a high-review dependency. Review advisories monthly, review versions quarterly, and require explicit approval plus the complete browser/WebView/security suite for any update. Read-only rendering does not promise preservation or editing of bookmarks, attachments, forms, annotations or signatures.
