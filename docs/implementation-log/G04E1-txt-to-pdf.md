# G04E1 TXT-to-PDF Implementation

## Scope and base

This lane implements exactly `text.to-pdf@1.0.0` on `codex/ds-g04e1-txt-to-pdf-v1`, starting from accepted main `a664dea4755122deefceac9d6ef699a920acc398`. It accepts one local regular non-reparse `.txt` input, strict UTF-8, A4 or Letter and portrait or landscape, then publishes one verified PDF without overwrite.

No database migration is added. Markdown, user HTML/CSS, CSV, JSON, XML, email, URL capture, Office conversion, batch execution, G04D, G04F2, G05, a system font, runtime download, HTTP server, shell path, package, tag, release and deployment are outside this slice.

## Dependency and font admission

The Windows target directly names `webview2-com = "=0.38.2"` and `windows = "=0.61.3"` with default features disabled and only `Win32_System_Com` and `Win32_UI_Shell`. Both packages and checksums already existed in the accepted lockfile; the lock delta adds only the two direct edges to the Document Studio package. No direct `webview2-com-sys`, `windows-core`, Git/path dependency or second COM helper is added.

Three unmodified static hinted TrueType files are packaged with exact OFL-1.1 materials. `font-manifest.json` records their names, versions, sizes, SHA-256 values, official repository/tag/commit/archive provenance, archive hashes, cmap evidence and OpenType table inventories. Preflight validates exact inventory, bytes, names, Regular weight 400, non-variable sfnt structure, required tables and cmap membership before WebView2 creation. Nothing is installed into Windows.

## Input and canonical document boundary

Rust retains a read-only native handle and private identity/size/modification/SHA-256 evidence. React receives only opaque session/generation metadata and a safe display name. Raw input is limited to 8,388,608 bytes, 100,000 normalized logical lines and 65,536 UTF-8 bytes per line. Preflight rejects invalid UTF-8, unsupported BOMs, later U+FEFF, controls, bidi controls, noncharacters, unsupported code points, excessive combining/default-ignorable runs and invalid or unbounded Devanagari/Telugu joiner clusters.

Rust generates the complete HTML document. Escaped user text appears only inside one `<pre>`; the fixed CSP denies every class except same-origin style and fonts. A separate fixed CSS response defines `DocumentStudioText`, weight 400, no synthesis or generic/system fallback, exact Unicode ranges, 11 pt/1.45/pre-wrap/anywhere/tab-size 4 and 0.5-inch print margins. Checked response caps and `u32::MAX` checks apply before COM response creation.

## Hidden renderer and no-network boundary

The renderer owns one dedicated STA/message-pump thread, hidden non-activating native window, controller and unique generation/UDF beneath the job workspace. It consumes the accepted SEC1C environment policy, never uses the primary application UDF and permits at most one active text render.

Before exact navigation it requires `ICoreWebView2_22`, registers a global all-context/all-request-source filter and response handler, installs navigation/window/download/permission/authentication/certificate/process/key denials and disables script, web messages, host objects, development UI, dialogs, status, zoom and accelerators. Every exact expected GET receives an application response; wrong methods receive empty 405 and every other URL/context/source receives empty 403 and terminates the generation. There is no pass-through, listener, loopback, localhost, file URL, data navigation, custom protocol, proxy fallback or external resource path.

Responses use generated COM projections, `SHCreateMemStream`, typed `IStream` and `IStream::Stat`. Immutable bytes, stream and response objects are retained through the response lifecycle. Navigation completion, native DOM-content-loaded and completed expected response events are required before printing; no JavaScript or `ExecuteScript` readiness probe exists.

## Print, verification, publication and recovery

`ICoreWebView2Environment6::CreatePrintSettings` and `ICoreWebView2_7::PrintToPdf` write only to private raw staging with exact paper dimensions, 0.5-inch margins, scale 1.0, backgrounds on and headers/footers off. Generation-bound callback authority includes job, generation, one-shot token, lifecycle version and cancellation; stale, duplicate and cancelled callbacks cannot publish or mutate another owner.

Accepted qpdf 12.3.2 runs inside the existing zero-capability AppContainer and owned Job Object. It creates a metadata-removed page-only normalized staging PDF, then strict-checks encryption, page count, page boxes/rotation, active content, actions, names, forms, annotations, embedded/external data, privacy canaries and exact embedded `FontFile2` inventory. Source evidence is rechecked before no-overwrite publication. The final path is reopened and must equal normalized staging size and SHA-256 before terminal success.

Cancellation reconciles only exact generation-owned workspace/UDF/staging data. Startup never resumes WebView2, printing or qpdf; it preserves proven publication, interrupts unfinished work and accepts cleanup only for exact job/generation markers. A published user file is never removed by cancellation or cleanup failure. WebView2 runtime version is recorded in private job evidence and a sanitized dependency diagnostic without a new schema field.

## UI and accessibility

The Convert workspace adds a TXT tab without removing existing conversion or batch surfaces. It explains local/offline processing, strict limits, fixed Regular fonts and supported English/Hindi/Telugu; provides labelled page/orientation, destination and exact output-name controls; warns about no-overwrite collision naming; reports bounded stage/progress/cancellation; and exposes verified result actions to reopen the unchanged output in the internal Viewer, focus/reveal the saved location in-app without an operating-system shell, and copy the saved path. Focus restoration, a dedicated bounded live announcement, native labels/descriptions, keyboard controls, existing visible-focus/zoom/reduced-motion/forced-colors token rules and no color-only state are retained. No document preview or full text is held in React.

## Focused implementation evidence

The immediate compile gate names `ICoreWebView2_22`, `ICoreWebView2Environment6`, `ICoreWebView2_7`, `ICoreWebView2WebResourceResponse`, resource and print handlers, `IStream`, `SHCreateMemStream`, resource-response creation/filtering, print settings and `PrintToPdf` from the production crate. Locked `cargo check`, focused Rust/contract/component tests and the G04E1 boundary verifier pass.

The first real hidden-renderer run produced a PDF before font responses were proven consumed and correctly failed `TXT_PDF_FONT_MISSING`. A native compositor-probe experiment then timed out while the controller was intentionally invisible and failed `TXT_RENDERER_TIMEOUT`; it was removed. The implemented repair uses native DOM plus `WebResourceResponseReceived` completion accounting. The resulting unchanged-policy smoke passed mixed English/Hindi/Telugu literal-text rendering for A4/Letter portrait/landscape through real WebView2 151.0.4129.107, qpdf normalization, strict security/font/page verification, no-overwrite publication, final reopen and exact owned cleanup. Both failed attempts and the final pass are retained as truthful task evidence.

Two existing regression harness isolation defects were found during final local validation and repaired without weakening product assertions. The first browser run omitted the lane `CARGO_TARGET_DIR`; its feature-worktree `target` was removed with an exact `cargo clean`, and the unchanged browser suite then passed 22/22 using the lane Cargo and Playwright caches. The first single-instance run passed a PowerShell-null CDP variable as an empty string and the product correctly stopped with `InvalidPort`; the harness now removes null-or-empty test variables, after which the normal case and all five injected failures passed with complete cleanup. The raw WebView2 harness now accepts an explicit evidence base and honors `CARGO_TARGET_DIR`. The optimized SEC1C hostile-environment proof used a test-only bundle identifier and owned profile, passed two launches, removed those exact process-free profile directories, and restored the exact production configuration without accessing the primary application profile.

The final callback audit also closed a stale-generation fail-closed edge: a late resource request now receives an application-generated empty 403 before its exact stale controller is closed, while permission, authentication, certificate, download, new-window and accelerator events are always natively denied or handled even after invalidation. Only the still-current generation may mutate lifecycle state.

Final manual review added an owning-STA controller close guard for every early-return path, moved verified-result Viewer hashing behind its retained write/delete-blocking handle, released TXT sessions and destination grants at the worker/UI ownership boundaries, and connected one verified native A4 output to the existing PDF.js range/canvas/text-layer acceptance harness. The PDF.js extraction assertion is representative and informational; it does not claim exact extraction equality or byte-identical output across WebView2 versions.

Repository-wide tests, exact-head CI and independent review are recorded in the PR and final owner-gate report after they complete; this log does not pre-claim them.

## Release state

The PR must remain unmerged until local acceptance, exact-head CI and independent exact-head review are complete. The branch and worktree are preserved for the owner merge gate.
