# Dependency and License Register

The dependency register distinguishes **adopt**, **evaluate**, **optional service** and **inspiration only**. Versions are verified at implementation time and pinned with checksums.

| Component | Use | Status | License/notes |
|---|---|---|---|
| Tauri 2.11.x | Desktop shell and secure IPC | Adopt | MIT/Apache-2.0; CLI 2.11.4 and JS API 2.11.1 were current at the 22 July 2026 recheck |
| React 19.2.7+ | UI | Adopt | MIT; pin a patched 19.2.x or later |
| Vite 8.1.5+ | Front-end build | Adopt | MIT; Vite 8 uses Rolldown; pair with `@vitejs/plugin-react` 6.x |
| PDF.js 6.2.108 | Viewer rendering and text layer beneath Document Studio UI | Adopt for G03 | Apache-2.0; exact npm pin, local worker/assets, no generic viewer UI |
| qpdf 12.3.2 | Structural PDF merge and verification | Adopt for G02 | Apache-2.0; signed checksum, exact archive/runtime hashes and bundled license materials verified |
| libvips 8.18.2+ | Possible later image conversion/compression adapter | Evaluate for G04C or a separately approved renderer chain | LGPL-2.1-or-later; dynamic-linking/distribution and every native loader/renderer require independent review |
| OCRmyPDF 17.8.x | OCR orchestration | Evaluate as managed optional worker | MPL-2.0; native Windows needs Python, Tesseract and Ghostscript; packaging and sandboxing gate required |
| Tesseract 5.5.2+ | OCR engine/language packs | Adopt | Apache-2.0; Windows installer is third-party, so binary provenance must be governed |
| LibreOffice 26.2.4+ | Office conversion | Adopt external dependency | MPL-2.0 and bundled notices; detect installed version and isolate profile directories |
| Gotenberg 8.32+ | Server-side Office/HTML conversion | Optional web service | MIT; container deployment; keep patched because conversion surfaces process untrusted files |
| pdf-lib | Browser overlays/simple creation | Evaluate/adopt for narrow cases | MIT; not full arbitrary editing/decryption |
| pdfcpu 0.12+ | Go-side PDF operations/signature evaluation | Evaluate | Apache-2.0; project describes itself as evolving |
| veraPDF | PDF/A validation | Evaluate | dual-license/distribution review required |
| Docling | Structured document parsing | Evaluate | MIT; model licenses separate |
| Granite Docling 258M | Local document structure extraction | Evaluate | Apache-2.0; English model |
| multilingual-e5-small | Multilingual embeddings | Evaluate | MIT; 512-token chunk limit |
| UI UX Pro Max | Design guidance for Codex | Adopt as development skill | MIT; install through `uipro-cli` |
| Tokens Studio | Figma design-token sync | Recommend | MIT |
| Iconify | Icon exploration/import | Recommend with icon-set license review | Plugin/repository terms plus individual set licenses |
| Design Lint | Figma consistency checks | Recommend | MIT |
| Stirling-PDF | Feature inspiration/benchmarking | Inspiration only | Do not vendor or copy; separate open-core/security considerations |

## Adoption gate

A dependency is not production-approved until licensing, update channel, binary provenance, sandboxing, performance, failure behavior and output verification are documented in an ADR.

## Version-policy note

The versions above are recheck baselines, not permanent pins. Before each release, update the dependency lock, verify upstream signatures/checksums and advisories, run the compatibility matrix, and record the adopted revision in an ADR and software bill of materials.

## G01 foundation adoption

[ADR-006](adr/ADR-006-foundation-dependencies-and-sqlite.md) approves the following direct foundation dependencies. Manifest policies are intentionally narrow and the committed lockfiles are authoritative for exact resolved versions and registry integrity. Transitive packages enter only through those reviewed lockfiles.

| Package or crate | Role | Manifest policy | Licence | Provenance | Necessity and lighter alternative | Surface |
|---|---|---|---|---|---|---|
| `@document-studio/contracts` | Shared TypeScript contracts and schemas | Exact workspace `0.1.0` | First-party proprietary | This repository | Prevents frontend contract duplication; relative imports are lighter but brittle | Production |
| `@document-studio/tokens` | Canonical design-token JSON export | Exact workspace `0.1.0` | First-party proprietary | This repository | Enforces token consumption; relative JSON imports are lighter but bypass the package boundary | Production |
| `@tauri-apps/api` | Typed invoke and event APIs | `~2.11.1` | MIT OR Apache-2.0 | Official npm registry/Tauri repository | Required bridge; raw internal IPC would be less safe | Production |
| `@tauri-apps/plugin-dialog` and `tauri-plugin-dialog` | Restricted native input/destination selection | `~2.7.2` | MIT OR Apache-2.0 | Official npm/crates.io/Tauri repository | Avoid unsafe path text entry; manual path entry is lighter but error-prone | Production |
| `tauri-plugin-single-instance` | Enforce one Windows process before database/worker setup | `~2.4.3` (locked `2.4.3`) | MIT OR Apache-2.0 | Official crates.io/Tauri plugins repository | Process-local cancellation requires an enforceable owner; custom Win32 mutex/message plumbing would duplicate security-sensitive official code | Windows production; no webview capability, network, shell or filesystem permission |
| `react`, `react-dom` | Desktop webview UI and renderer | Matching `~19.2.7` | MIT | Official npm registry/React repository | Approved UI stack; vanilla DOM is lighter | Production |
| `@tauri-apps/cli` | Development/build commands | `~2.11.4` | MIT OR Apache-2.0 | Official npm registry/Tauri repository | Required smoke/build tooling; a global CLI is less reproducible | Development |
| `typescript`, `vite`, `@vitejs/plugin-react` | Strict compile and frontend build | `~5.8.0`, `~8.1.5`, `~6.0.3` | Apache-2.0 or MIT | Official npm packages | Approved starter toolchain; custom JavaScript/bundling is lighter only in package count | Development |
| `@types/react`, `@types/react-dom` | React type declarations | `~19.2.18`, `~19.2.4` | MIT | DefinitelyTyped npm packages | Required strict typecheck; handwritten declarations are not maintainable | Development |
| `vitest`, `jsdom` | Real TypeScript/frontend tests and DOM environment | `~4.1.10`, `~30.0.1` | MIT | Official npm packages | Replaces zero-test evidence; custom loaders/DOM mocks are lighter but fragile | Tests |
| Testing Library packages | Role/label and keyboard/pointer interaction tests | `~16.3.2`, `~10.4.1`, `~14.6.4` | MIT | Official npm packages | Tests behavior/accessibility; raw DOM events are lighter | Tests |
| `axe-core` | Automated accessibility smoke | `~4.13.0` | MPL-2.0 | Official npm package | Finds common WCAG/ARIA defects; manual-only checks remain required but are not reproducible | Tests |
| `ajv`, `ajv-formats` | Draft 2020-12 schema and UUID/date-time validation | `~8.20.0`, `~3.0.1` | MIT | Official npm packages | Makes contracts executable; handwritten validation is lighter but incomplete | Tests |
| `tauri`, `tauri-build` | Desktop runtime and generated application context | `~2.11.5`, `~2.6.3` | MIT OR Apache-2.0 | crates.io/Tauri repository | Approved desktop boundary; another framework is out of scope | Production/build |
| `serde`, `serde_json` | Typed IPC, settings, and repository payloads | Compatible `1.x`, exact lock | MIT OR Apache-2.0 | crates.io | Required Tauri serialization; manual serializers are less safe | Production |
| `uuid` | Unpredictable job/workspace IDs | Compatible `1.x`, v4/serde, exact lock | MIT OR Apache-2.0 | crates.io | Avoids user-derived workspace names; custom IDs are unsafe | Production |
| `thiserror` | Internal typed error mapping | Compatible `2.x`, exact lock | MIT OR Apache-2.0 | crates.io | Keeps raw errors behind safe envelopes; manual traits are lighter | Production |
| `rusqlite` plus bundled SQLite | Metadata repository and migrations | `~0.40.1`, `bundled` | MIT; SQLite public domain | crates.io and SQLite upstream source | Small embedded database; SQLx adds runtime/macros and raw FFI is unsafe | Production |
| `chrono` | UTC RFC3339 timestamps | `~0.4.45`, reduced features | MIT OR Apache-2.0 | crates.io | Contract date-time strings; manual formatting is lighter but error-prone | Production |
| `sha2` | Streaming SHA-256 fingerprints/verification | `~0.11.0` | MIT OR Apache-2.0 | crates.io/RustCrypto | Required independent output proof; OS crypto bindings add platform coupling | Production |
| `windows-sys` | Reparse/file-identity/no-replace/flush/ACL Windows APIs | `~0.61.2`, minimum Win32 features | MIT OR Apache-2.0 | crates.io/Microsoft windows-rs | Required Windows path/publication controls; shell commands are prohibited | Windows production |
| `tempfile`, `junction` | Isolated filesystem/database fixtures and non-admin junction tests | `~3.27.0`, `~2.0.0` | MIT OR Apache-2.0; MIT | crates.io | Makes cleanup and reparse tests reliable; handwritten temp paths or skipped tests weaken evidence | Tests |
| `PyYAML` | Parse the existing model registry validator input | Exact `6.0.3` with hashes | MIT | PyPI | Existing validator requirement; a custom limited YAML parser is lighter but unnecessary | Validation |
| `pip-tools` | Generate the hash-pinned validation lock | Exact `7.6.0` when invoked | BSD-3-Clause | PyPI | Reproducible hashes; hand-maintained hash lists are error-prone | Implementation tooling |

G01 deferred production document engines. G02 adopted only qpdf 12.3.2. G03 adopts only the PDF.js/virtualization/browser-test packages listed below; libvips, OCRmyPDF, Tesseract, LibreOffice, Ghostscript, Go services, model runtimes, cloud/provider SDKs, SQLCipher, and general Tauri filesystem/shell plugins remain deferred. Dependency diagnostics never install or download an engine. The single-instance plugin is Rust-side only and requires no capability-file change.

## G02 qpdf adoption

[ADR-009](adr/ADR-009-qpdf-and-production-pdf-merge.md) adopts only qpdf 12.3.2 for page-only PDF Merge. The official `qpdf-12.3.2-msvc64.zip` is pinned to 24,555,583 bytes and SHA-256 `8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202`. Its checksum bundle verifies through Sigstore for `ejb@ql.org` and issuer `https://github.com/login/oauth`.

The reviewed resource contains `qpdf.exe`, `qpdf30.dll`, the required Microsoft Visual C++ 14.44.35211 runtime DLLs, full Apache-2.0 text, upstream qpdf license pages and signed provenance. Fifteen controlled files have hard-coded relative paths, sizes and SHA-256 hashes; the resource manifest makes sixteen files and 8,574,799 bytes total. No PATH lookup, installer, registry change, runtime download, updater or alternate engine is allowed.

Cosign 3.0.6 is acquisition/CI verification tooling, not a shipped application runtime. `windows-sys` remains the existing Rust dependency; G02 only enables its AppContainer, token, process and Job Object feature modules, so no Cargo package version or lockfile entry changes.

## G03 viewer and browser-test adoption

All installs used exact versions plus `--ignore-scripts`, so no lifecycle script executed. The lockfile may still record optional platform-package install-script metadata such as `fsevents`; that metadata is not claimed absent. The lockfile contains registry URLs and Subresource Integrity; no runtime package fetch occurs.

| Package | Exact/locked version | Licence | Official registry integrity | Role and boundary |
|---|---:|---|---|---|
| `pdfjs-dist` | 6.2.108 | Apache-2.0 | `sha512-YxFb+SQcodN2rnX9Tn3dHYlqfb7NjlzzfONPpJd+AKoKtUjEdevTfbC07d5TcczzOK6261auRkP/M8OBHs9vFQ==` | Production display API/worker from Mozilla PDF.js; 550 files/34,497,725 bytes unpacked; published 2026-07-28; no wrapper/generic UI |
| `@tanstack/react-virtual` | 3.14.9 | MIT | `sha512-qZyr0FZDP8rDC4WBhsryIZmAd9bveJvFGUJJtskWaew6/0dTRS6wZxnR6VQ5bY2KwL3LjerrHqQLk3a0GKcPXQ==` | Production page/thumbnail virtualization; 9 files/56,532 bytes unpacked; published 2026-07-28 |
| `@tanstack/virtual-core` | resolved 3.17.7 | MIT | `sha512-bp+v10y65sp2H7WpWfIMyxTNfl8ZVfxFTLRjPIFRryi6FV/J33z4IS53WO4pTk36KlvJ4iLiQz+oaydDC1xbcA==` | Transitive virtualizer core; do not force a predicted version |
| `@playwright/test` | 1.62.1 | Apache-2.0 | `sha512-DTcUc8qii+cpHvtOwggMtBRMjKZHXYWdw8syRYu2vtzuq4Wxphqq4NfCs5Zt44L6mA8rfDfj+PHnxFc/FeK6mQ==` | Development/test only; published 2026-07-30 |
| `playwright` / `playwright-core` | resolved 1.62.1 | Apache-2.0 | Lockfile-controlled | Transitive test runner/browser automation |
| `@napi-rs/canvas` platform set | resolved optional 1.0.6 | MIT | Lockfile-controlled | Optional `pdfjs-dist` Node canvas dependency; not imported by the WebView viewer |

The project-local Playwright acquisition contains Chrome for Testing and Headless Shell 151.0.7922.34 at revision 1234, ffmpeg revision 1011 and winldd revision 1007 under `.cache/ms-playwright/`; it is ignored, not system-wide, and not shipped. CI downloads only this locked test browser. Production remote debugging is absent.

The PDF.js staging script checks package/API/worker version parity and copies only the 191 files in `apps/desktop/scripts/pdfjs-assets-6.2.108.json`. It verifies every source and staged path, byte size and SHA-256 in a fresh sibling, proves exact membership, then atomically replaces the old directory with rollback. QuickJS, no-WASM fallbacks, maps, debug/test/example content and generic viewer UI are excluded. The application makes no CDN or network request. Security review is monthly, version review quarterly, and every update requires explicit dependency approval, lock/integrity/licence review, worker hash/parity proof and the complete browser/WebView/security suite.

## G04A lossless-compression adoption

G04A adds no new production dependency. `pdf.compress-lossless@1.0.0` reuses the accepted, exactly bundled qpdf 12.3.2 runtime, fixed AppContainer/Job Object process profile, independent verification and durable no-overwrite publication boundary from ADR-009. G04A is accepted on main merge `a27306653119e6e4fcdef162308445b78129f974`.

## G04B images-to-PDF adoption and renderer blocker

[ADR-013](adr/ADR-013-g04b-image-pdf-conversion-dependencies.md) accepts only the in-process writer dependencies below. All are exact crates.io versions with registry checksums in `Cargo.lock`; no default codec set, native executable, DLL, installer, runtime download, network call, PATH lookup, shell permission or system service is added.

| Crate | Exact version | Crates.io SHA-256 | Licence | Enabled role/features |
|---|---:|---|---|---|
| `image` | 0.25.10 | `85ab80394333c02fe689eaf900ab500fbd0c2213da414687ebf995a65d5a6104` | MIT OR Apache-2.0 | `default-features = false`; JPEG, PNG, WebP decoding, dimensions and orientation only |
| `pdf-writer` | 0.15.0 | `f5e456864a7a304047bff84977dc6fb162bd956475d40ba50b2dcecaada7f753` | MIT OR Apache-2.0 | `default-features = false`; original PDF object generation |
| `flate2` | 1.1.9 | `843fba2746e448b37e26a819579957415c8cef339bf08564fe8b7ddbd959573c` | MIT OR Apache-2.0 | `default-features = false`, `rust_backend`; deterministic image streams |

On 22 August 2026 the exact direct crates and their resolved codec/compression subtree had no exact-version matches from the GitHub Advisory API/RustSec data reviewed for this gate. This is point-in-time evidence; every update and release must repeat the advisory, checksum, licence and notices review.

`pdf.to-images@1.0.0` is not adopted. PDFium source does not supply the reviewed signed/versioned Windows runtime provenance required here; MuPDF requires an AGPL-compliant distribution or commercial licence; libvips does not remove the need to approve its PDF renderer chain; and PDF.js export needs a new durable raster architecture. The operation remains on the roadmap but is dependency-blocked with no production code or fallback.
