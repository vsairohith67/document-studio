# Security, Privacy and Threat Model

## Trust boundaries

- Untrusted documents and archives.
- UI/webview process.
- Tauri/Rust trusted core.
- Third-party document engines.
- Optional external AI/cloud providers.
- User filesystem and OS credential store.

## Required controls

- Allow-list executables and operation arguments.
- Pass arguments as arrays; never concatenate shell strings.
- Canonicalize paths and enforce destination/workspace roots.
- Reject path traversal, symlink escape and dangerous archive entries.
- Isolate each job; use least privilege and resource limits where supported.
- Validate magic bytes and parser output, not only extensions.
- Patch bundled engines and record exact versions/hashes.
- Treat PDFs, images, fonts, Office files and archives as attacker-controlled.
- Disable network by default for document processing.
- Require per-job consent for external processing and show provider/data scope.
- Never log passwords, keys, document text or raw model prompts by default.
- Verify signed updates and bundled sidecar checksums.

## G01 Windows controls

- Accept one existing regular local-disk input and one user-selected local destination for `diagnostic.copy`.
- Reject UNC/device namespace paths, alternate data streams, reserved DOS names, traversal, trailing-dot/space names and every reparse component.
- Compare Windows file identities so the destination cannot alias the input through a hard link.
- Keep each job under a random UUID workspace with an application ownership marker. Cleanup verifies the canonical application root, UUID and marker before exact removal.
- Open the source with sharing that denies mutation/deletion while copying. Stream fixed-size chunks and never load the whole document.
- Before creating or writing a destination-local random partial, atomically record its exact direct-child path and final candidate as a reservation. A reservation alone never authorizes deletion.
- Create a separate random guard with Windows delete-on-close and `create_new`, capture its file identity, and durably activate that identity for the exact reserved partial UUID. Then create the partial as a no-replace hard link, reopen it, verify the same identity, and only then write document bytes. A crash before activation removes the guard; a crash before the link leaves no partial.
- Cleanup opens only the exact recorded `1.0.1` path and deletes through that handle only when its current Windows identity matches the durable activation token. Missing or identity-mismatched paths clear only the reservation and preserve any unknown file. Cleanup never scans/globs `.document-studio-*`, infers ownership from names, or removes final/neighboring files.
- Flush, close, independently hash, then move in the same directory with no-replace/write-through flags and reopen the final for proof.
- Resolve collisions as `name-copy.ext`, then numbered suffixes, with at most 1,000 attempts. A commit-time race preserves the competing file; cleanup failure stops retries and leaves durable interrupted ownership.
- Allow cancellation between bounded chunks and stages. The final no-replace move is a small, explicit non-cancellable commit boundary.
- Complete only after verification, durable publication evidence, audit persistence and confirmed temporary-data cleanup. Ambiguity remains `interrupted`.
- Treat `diagnostic.copy` `1.0.0` as pre-fix compatibility data. Null paths and historical terminal states are not cleanup proof; ambiguous records and unknown files are preserved with `LEGACY_CLEANUP_UNPROVEN`.
- Register the official single-instance plugin before every other Tauri plugin. Its identifier-derived Windows mutex prevents a secondary process from reaching database/worker setup; forwarded arguments and working directory are ignored, and only the existing main window is restored/focused.
- Expose only Tauri core defaults and the native open-dialog permission. G01 has no shell, general webview filesystem or HTTP capability.

Tests place document and password sentinels in inputs and confirm they do not appear in SQLite, events, errors or dependency diagnostics.

## G02 qpdf process boundary

- Use only the manifest-verified bundled qpdf 12.3.2 executable and sibling DLLs. Never discover qpdf through PATH and never call a shell.
- Use the stable `DocumentStudio.PdfEngine.Qpdf.V1` production AppContainer profile with zero capabilities. Create or derive it idempotently, verify the derived SID and configuration contract, and fail closed on mismatch. Tests use and delete only the separately named fixed test profile.
- Create qpdf suspended, validate its token as the exact zero-capability AppContainer, assign it to a private Job Object, query back kill-on-close/one-process/2 GiB limits, and only then resume it.
- Grant the AppContainer only the verified engine cache and the current owned job paths. A test proves writes inside the grant and denial of an ungranted file.
- Forward `SystemRoot` and `WINDIR`; map `LOCALAPPDATA`, `TEMP` and `TMP` to the private job temporary directory. Do not forward the real user profile, proxy, cloud or qpdf environment.
- Prove loopback connection denial. Cancellation and timeout terminate only the retained owned Job Object; an unrelated-process test must remain alive.
- Run the exact page-only merge argument vector with no production `--deterministic-id`, then reopen and independently verify the staging result before G01 publication can begin.
- Continuously drain stdout/stderr but retain only bounded 64 KiB in-memory tails. Raw engine output is never persisted or shown to the user.
- Reopen the published result and require its size and SHA-256 to equal the verified staging evidence. Restart recovery never resumes qpdf and deletes only marker/identity-proven owned artifacts.

## G03 hostile-viewer boundary

Assets at risk are local PDF contents, paths, passwords, published outputs and application authority. The attacker may supply a malformed/encrypted PDF, PDF actions/links/attachments/forms, huge images/text layers, range floods, a replaced path, stale session/grant, reparse/hard-link alias, or attempt navigation/data exfiltration from the webview.

Controls are defense in depth:

- Rust validates an existing regular non-reparse local PDF and retains a read-only Windows handle that denies write/delete sharing. Positioned reads revalidate opaque session/generation, bounds and source identity; there are at most two sessions, four reads per session and 1 MiB per request.
- Backend open and Rust-side Tauri drop keep the path out of React, PDF.js, custom events, logs and errors. Destination grants are opaque and revocable. No `file://`, custom protocol, HTTP server or broad filesystem capability exists.
- PDF.js is exact-version/local-only with a same-origin worker and an exact checked-in asset path/size/hash manifest; atomic staging preserves no stale files and has no network fallback. Loading/configuration sets `isEvalSupported: false`, `enableXfa: false`, `renderForms: false`, `stopAtErrors: true`; page rendering disables annotations and Document Studio creates no interactive form/annotation layer, scripting manager, attachment UI or external navigation.
- CSP allows only self, required data/blob images/fonts, inline styles, same-origin workers and WebAssembly compilation through `wasm-unsafe-eval`; it denies `unsafe-eval`, external connects, objects, bases, frames and forms. Rust blocks outside-origin navigation/new windows.
- Page/full-resolution canvas counts, image pixels, DPR, text items/chars/results and indexing/range concurrency are bounded. Obsolete RenderTasks/searches are cancelled and close destroys PDF.js/worker/session resources.
- Viewing passwords remain only in component memory and are never logged/persisted. Encrypted structural operations fail before qpdf processing; there is no qpdf password transport or process command-line password.
- Durable operations snapshot only from the retained handle, use ASCII private paths/direct qpdf argv, verify every result and preserve the accepted sandbox/publication/recovery controls.
- G03 durable records retain paths only inside the trusted Rust/database boundary. Every G03 create/get/history/recovery response strips source, canonical, destination, staging, partial and final paths before serialization; a regression proves G01/G02 job responses are unchanged.

Residual risk: PDF.js and WebView2 are complex native/browser parsers, `style-src 'unsafe-inline'` is required by PDF.js text layout, and `wasm-unsafe-eval` permits WebAssembly compilation. Exact pinning, offline packaging, CSP/navigation tests, monthly advisory review and update gates reduce but do not eliminate parser risk. Unsupported document features are not described as preserved or sanitized.

## Redaction safety

Visual cover-up is not redaction. The operation must remove underlying text/objects or rasterize/sanitize the affected area, then test extraction and visual output. The UI requires a final irreversible confirmation.

## Privacy modes

- Normal local mode.
- Strict offline mode (all network disabled, including update/model checks).
- External-provider mode enabled per feature/provider with explicit scope.
