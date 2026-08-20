# G03 Viewer and Core PDF Implementation Log

Status: Draft PR #6; WebView2 CI smoke remediation locally verified
Accepted base: `6e96b394eba7fafe787920e9d6bdd0c4b99f2670`
Implementation branch: `feat/g03-viewer-core-pdf`
Target: Windows x64, Tauri 2, WebView2, PDF.js 6.2.108 and qpdf 12.3.2

## Plan-ready synchronization

- Updated the existing Notion G03 page `3bdc9801-27a8-812c-bf27-cef7f57da017`, kept its status Planning and verified the PLAN READY title by read-back.
- Updated existing incomplete Asana task `1217527657971162` and verified it remained incomplete.
- Updated only existing Canvs element `fMN31pCJ5JSlRAk53FZwW` in room `odUF4mDxzT5PNo-3697W`.
- Created no GitHub planning object, duplicate tracker or board.
- Verified clean `main` at the accepted base before creating the approved branch.

## Approved dependencies and local browser

- Installed exact runtime dependencies `pdfjs-dist@6.2.108` and `@tanstack/react-virtual@3.14.9`, plus exact development dependency `@playwright/test@1.62.1`, using `--ignore-scripts`.
- Lock resolution retained those direct versions exactly and resolved `@tanstack/virtual-core@3.17.7`, `playwright@1.62.1` and `playwright-core@1.62.1`. No unrelated direct dependency changed and the final locked audit reported zero vulnerabilities.
- The official npm metadata records PDF.js as Apache-2.0, 34,497,725 unpacked bytes/550 files; TanStack React Virtual as MIT, 56,532 bytes/9 files; and Playwright Test as Apache-2.0, 28,544 bytes/11 files. Exact registry integrity strings are recorded in ADR-010 and the dependency register.
- The approved project-local Playwright acquisition installed Chrome for Testing 151.0.7922.34 revision 1234, matching headless shell, FFmpeg revision 1011 and winldd revision 1007 below `.cache/ms-playwright`. The 4,024,832-byte `chrome.exe` SHA-256 is `409805a16d6416087e6b2f778df1cf8f7bbb267d6b99f6b5bb0a618eace234f2`. It installed no system browser and the cache is ignored.

## Database and plan contract

- Migration 4 adds only `job_operation_plans`, with one plan per job, schema version 1, source page count, canonical JSON, SHA-256 and the exact `length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536` database constraint.
- Rust independently enforces 2–65,536 UTF-8 bytes, canonical serialization, hash equality, operation matching and a fresh qpdf source-page count before execution.
- Existing G01/G02 jobs remain valid without plans. No document bodies, thumbnails, extracted text or passwords enter the new table.
- Public `1.0.0` manifests and shared TypeScript/Rust contracts cover `pdf.extract-pages`, `pdf.remove-pages`, `pdf.reorder-pages`, `pdf.rotate-pages` and `pdf.split`. UI page labels remain 1-based; persisted page indexes are explicitly 0-based.

## Opaque viewer sessions and raw range IPC

- The backend native Open PDF dialog and Rust-side Tauri drag/drop handler validate one regular non-reparse PDF before creating an opaque session. The custom frontend event contains sanitized metadata only and never the dropped path.
- A retained Windows read-only handle denies write/delete sharing. Positioned reads use the handle without a shared seek cursor; repeated and overlapping reads are valid.
- `viewer_read_range` returns `tauri::ipc::Response` raw bytes, not JSON arrays or base64. PDF.js requests normal 256 KiB chunks; Rust rejects more than 1 MiB and permits four reads in flight per session.
- Sessions are limited to two, use session ID plus generation, revalidate source identity/size/time, expire, cancel, invalidate stale responses on Close, and close on window/application shutdown. The retained handle also creates the private durable-job snapshot.
- Opaque destination grants expose no directory path. Existing IPC v1, G01/G02 commands and `dialog:allow-open` remain present.
- Durable G03 records retain trusted paths only inside Rust/SQLite. G03 create/get/history/recovery IPC redacts source, canonical, destination, staging, partial and final path fields; G01/G02 response shapes and values remain unchanged.

## PDF.js packaging and security

- The local staging script verifies API/worker version parity and stages only the legacy ESM worker, CMaps, standard fonts, ICC profiles and WASM. The staged 202-file set is 5,892,091 bytes; worker SHA-256 is `b4e582882f5e811f4d1b7b511f68d9a0c3209141e6f68856f01408c5cc155131`.
- There is no CDN, runtime fetch fallback, React PDF wrapper or copied generic viewer. The production UI is Document Studio's Precision Paper workbench.
- Explicit PDF.js settings include `isEvalSupported: false`, `enableXfa: false`, `renderForms: false` and `stopAtErrors: true`. The app creates no scripting manager, annotation layer, form UI, attachment UI or external navigation.
- The exact approved CSP permits only same-origin scripts/workers and required local/data/blob image/font assets, includes `wasm-unsafe-eval`, and denies objects, bases, frames and form actions. Rust blocks external navigation and new windows.
- Password entry exists only in React component memory and is cleared on Close. It is never logged, persisted or passed to qpdf; encrypted structural operations remain rejected.

## Progressive viewer, search and organizer

- TanStack virtualizers mount visible pages/thumbnails plus small overscan only. Mixed page sizes are measured; first page and visible thumbnails are prioritized; obsolete PDF.js RenderTasks are cancelled.
- Page canvases cap DPR/pixel area. Eviction zeros canvases, cancels text layers and releases mounted resources. Close destroys the PDF.js loading task/document/worker transport and opaque Rust session deterministically.
- Navigation covers first/previous/next/last, page number, Page Up/Down, Home/End, actual size, fit page, fit width, zoom shortcuts and temporary view rotation.
- Search uses Unicode NFKC case-folding, two bounded extraction tasks, per-page item/character limits, a 16-million-character memory cache and a 100,000-result cap. It starts before full indexing, yields between page extractions, prioritizes visible pages, cancels on query/session change and persists nothing. Image-only pages truthfully report that OCR is unavailable.
- The shared organizer keeps selection, reorder, remove, output rotation and split planning ephemeral. It creates a durable job only when Apply/Export is pressed. Pointer and keyboard reorder retain stable focus; all icon buttons have accessible names.

## Five core operations

- Extract exports a nonempty unique page list in selected order.
- Remove exports the ordered complement and rejects removal of every page.
- Reorder requires an exact source-page permutation.
- Rotate flattens existing rotation, then applies unique 90/180/270-degree clockwise page rotations; temporary viewer rotation never enters the output plan.
- Split accepts every-page, fixed-count or explicit ranges after they become a 1–128-output, uniquely named, full-document non-overlapping partition.
- Every operation uses the accepted qpdf 12.3.2 binary, fixed zero-capability AppContainer, Job Object, direct argv, ASCII private snapshot/staging paths, timeout, cancellation and 64 KiB output tails. qpdf receives no original user path and never writes the source.
- Each output must be an owned regular staging PDF with magic, nonzero size, SHA-256, strict qpdf no-recovery structure, unencrypted state, exact page count, plan invariants and expected rotation where applicable. Final publication must match staging size/hash. Runtime does not claim arbitrary visual page identity; deterministic adversarial fixtures prove semantic order.

## Multi-output publication and recovery

- Every expected `job_outputs` row exists before processing begins. Split stages and verifies every output before publication starts.
- Publication remains a truthful sequential filesystem operation. Completed requires every expected output to be published with matching final evidence.
- A later publication failure records failed/`PARTIAL_PUBLICATION`, retains truthful output-row state and never deletes an already published user file.
- Cancellation cannot rewrite a committed publication result. Restart never resumes qpdf; it reconciles valid publication evidence and cleans only proven-owned private/unpublished artifacts while preserving neighboring and identity-mismatched files.

## Measured evidence

The reference machine was Windows 11 Home build 26200, Intel Core i7-11800H (8 cores/16 logical processors), 16,866,865,152 bytes RAM and Samsung MZVL21T0HCLR NVMe. Browser evidence used project-local Chrome 151.0.7922.34; native evidence used WebView2/Edge 151.0.0.0. Full cold/warm tables and fixture qualifications are in the performance architecture.

- The 100/1,000-page real-Chromium suite retained at most three full canvases and ten thumbnails, then zero canvases after Close. The 1,000-page p95s were 190 ms visible-page render, 220 ms zoom, 125 ms first search result and 17.45 ms mean scripted scroll frame. Positive heap growth peaked at 12,337,796 bytes in the final run.
- Real WebView2 Tauri IPC on a 44-page/1,079,368-byte PDF measured session open 1.3/374.4 ms median/p95 (the p95 was the first cold Tauri invocation), a 256 KiB read 145.4/157.8 ms and close 1.1/2.1 ms. It proved raw bytes, sanitized metadata and stale-generation rejection.
- Five four-page adversarial operation samples measured complete processing/publication: extract 417.93/424.51 ms, remove 386.29/404.60 ms, reorder 394.36/420.62 ms, rotate 774.47/990.12 ms and three-output split 810.86/819.70 ms median/p95. Preflight was 178.32/198.80 ms and pre-execution cancellation/cleanup 13.39/14.14 ms.
- A native-window inspection opened the real 44-page blueprint through the backend Ctrl+O dialog, rendered progressive pages/thumbnails and selectable text, exposed only its display filename, and returned to the empty workspace after Close.

## Final verification evidence

| Gate | Result |
|---|---|
| Repository validator | Passed; 132 feature entries and G01/G02 compatibility verified |
| Internal link validator | Passed |
| Exact dependency tree and audit | Passed; approved direct/transitive versions, zero vulnerabilities |
| PDF.js local staging/boundary verifier | Passed before and after production build; exact worker parity/hash and production/test separation |
| TypeScript | Passed for desktop and shared contracts |
| Frontend/shared unit tests | Passed; 27 desktop plus 12 contract tests (39 total) |
| Real Chromium PDF.js tests | Passed; 9 tests including hostile, encrypted, damaged, image-only and 1,000-page cases |
| Frontend production build | Passed; one non-failing lazy viewer chunk-size warning retained |
| Rust format and clippy | Passed with warnings denied |
| Rust default tests | Passed; 113 tests, with only the pre-existing manual G02 benchmark ignored |
| Core-operation/recovery test-runtime suite | Passed; 6 operation plus 17 recovery tests |
| AppContainer/Job Object feature suite | Passed; 3 real boundary tests |
| Single-instance smoke | Passed; secondary exited before runtime setup |
| Real WebView2 raw IPC smoke | Passed; raw bytes, metadata allow-list and stale close proven |
| Tauri no-bundle release build | Passed; known `.app` identifier advisory and chunk warning only |
| Git/diff audit | Passed; clean index, accepted base/head, one approved migration, no capability/Cargo dependency change, no forbidden boundary or scope expansion |

## Ready-to-stage synchronization

- Notion page `3bdc9801-27a8-812c-bf27-cef7f57da017` reads back `G03 Viewer and Core PDF — READY TO STAGE`, Status Review and the final evidence summary.
- Asana task `1217527657971162` reads back the same name and remains incomplete.
- Canvs element `fMN31pCJ5JSlRAk53FZwW` in the existing room reads back the same text.
- No GitHub object, duplicate tracker or duplicate board was created.

## Remaining bounded evidence

- Physical Explorer-to-app drag/drop is a manual pre-PR check: the available GUI driver cannot drag across window boundaries. The compiled Rust Tauri drag/drop handler and validation/session tests are present; failure in real manual testing triggers the approved native-drop gate and does not authorize COM.
- Forced colors, 200% application zoom and host reduced-motion settings remain manual pre-PR accessibility evidence.
- The approximately 1 GiB image-heavy viewer corpus, 128-output split and 128-input preflight benchmarks remain clean-reference/release evidence. No performance claim is made for them now.

## WebView2 clean-runner remediation

- Push run `32352347074`, desktop job `96374039392`, reached the real WebView2 step after every earlier check passed, then Playwright received `ECONNREFUSED` from fixed port 9333. The committed harness had let its best-effort CDP polling loop expire and still invoked Playwright, so the immediate failure cause is proven; the historical log did not contain enough process evidence to claim why that runner never opened CDP.
- PR synchronize run `32371138533`, desktop job `96431745215`, proved the fresh profile and fail-fast harness worked on the clean runner, but the actual WebView2 browser process omitted both requested CDP switches. The test-runtime process and WebView2 children were alive, the runtime marker and unique profile were present, no listener appeared and Playwright was not invoked. This isolated the remaining defect to test-runtime window construction: inherited `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` was not a reliable CDP injection boundary.
- The final harness reserves a dynamic `127.0.0.1:0` TCP port and creates a fresh `<evidence>\app-data\webview2-user-data` directory for every invocation. Under `test-runtime` only, the generated Tauri context suppresses its configured `main` window in memory, then `WebviewWindowBuilder::from_config` creates exactly one replacement with the validated data directory and direct browser arguments. Locked WebView2 appends its deterministic `EBWebView` child directory; the harness verifies that exact browser command line and never touches a production profile.
- Direct builder arguments retain Wry's required `msWebOOUI`, `msPdfOOUI` and `msSmartScreenProtection` disables and add only the dynamic loopback debugging port plus its exact `http://127.0.0.1:<port>` allow-origin. Wildcard origins, `--user-data-dir`, non-loopback hosts and inherited WebView2 browser-argument injection are prohibited.
- Eight focused tests cover automatic-window mode, the valid CDP and no-CDP manual modes, missing/malformed ports, relative/out-of-bound/nonempty/reparse directories and the browser-argument deny list. Test directories must be absolute, empty, non-reparse children of isolated test app-data and outside the configured production profile boundary.
- Vite now serves and probes the exact configured `http://localhost:1420` URL. Deadline gates fail closed with `VITE_NOT_READY`, `DESKTOP_NOT_READY` or `WEBVIEW2_CDP_NOT_READY`; Playwright starts only after valid `/json/version` data and an owned loopback WebView2 listener are proven.
- Failure diagnostics are sanitized and bounded to the last 200 lines or 64 KiB from each Vite/desktop stream. Cleanup targets only the owned desktop/Vite trees and WebView2 processes using the unique test profile, restores test environment variables, removes that invocation's evidence and confirms the dynamic port has closed.
- Five normal reliability runs passed: three consecutive, one immediately after the ordinary automatic-window single-instance smoke, and one while prior port 65229 was occupied. Every run proved a raw 262,144-byte Tauri IPC response, no base64, the metadata allow-list, stale-session rejection, dynamic CDP ownership, the unique profile and complete cleanup. Vite-disabled and CDP-disabled injections produced the expected bounded failure codes, never invoked Playwright and left no smoke evidence or owned process behind.
- Production `tauri.conf.json` remains at reviewed SHA-256 `b97a850398a47fb391bae2f076ea7e37775c5fc88b6de264cadd5f38638e4217`, with one automatically created main window and the accepted CSP. The builder override and its environment names compile only with `test-runtime`; the production executable contains none of them and retains the remote-debug switch deny-list. Viewer, drag/drop, session, frontend, PDF.js, capability and navigation files were unchanged, so the accepted physical Explorer drop evidence remains valid.
- Full post-remediation acceptance passed: 39 frontend/shared tests, 9 real Chromium/PDF.js tests, 116 default Rust tests, 8 focused test-window tests, 19 focused database tests, 2 page-plan tests, 6 feature-gated operation tests, 17 recovery tests, 3 AppContainer/Job Object tests, qpdf bundle verification, the hardened WebView2 reliability matrix, production boundary scan and Tauri no-bundle release build.

## Scope and repository safety

- G03 added no custom protocol, COM drop, shell/HTTP/general-filesystem capability, runtime CDN, telemetry, extra migration, extra dependency or G04 feature.
- No staging, commit, push, pull request, merge or release action was performed during implementation.
- The protected external Excalidraw library was not accessed or modified.

## Incremental-search determinism remediation

- PR synchronize run `32376748676`, desktop job `96449897143`, exposed a race in the 1,000-page browser fixture: a visible page could be queued again while its text extraction was already active, and repeated page-result replacement increased the global count without first removing that page's earlier offsets. The single logical match could therefore be reported as `1 of 2`.
- Controlled deferred-text tests reproduced both failures before the production fix. One interleaving extracted source page 0 three times; a same-query reprioritization left one result-map entry with one offset but a global count of two.
- `PdfTextIndexer` now owns an explicit in-flight page set. Queued, cached, unavailable and active pages are deduplicated defensively while the existing concurrency limit of two, visible-page priority and event-loop yielding remain unchanged.
- Text extraction is document work rather than query-generation work. An extraction that began before a query change caches once and evaluates against the latest active query when it completes; cached pages are never re-extracted for later queries.
- Per-page replacement subtracts the previous offsets before adding the new offsets, so `resultCount` remains equal to the sum of all `resultPages` offset lengths across reprioritization, query changes, completion races, unavailable pages and result limiting.
- Thirteen focused search tests cover active-work deduplication, query changes during one or multiple extractions, idempotent replacement, 100 repeated priorities, image-only/unavailable pages, the 100,000-result limit, destroy-during-extraction and seeded completion orders 17, 41 and 97. Twenty consecutive focused runs passed (260 test executions).
- The 1,000-page Chromium test now targets `.search-bar [role="status"]`, requires exactly `1 of 1` within the original 15-second ceiling, emits bounded content-free diagnostics on failure and proves the result navigates to source page index 1 with the expected text layer.
- The focused 1,000-page Chromium case passed 50 consecutive executions with one worker and no retries. Three consecutive complete nine-test browser suites also passed (27 tests); their measured first-result p95 remained 85–109 ms and every close retained zero canvases.
- Full post-remediation acceptance passed: repository and link validation; desktop/contracts typecheck; 49 frontend/shared unit tests; production frontend build; Rust format, warning-denied clippy, 116 default tests and locked check; 19 focused database tests; 2 page-plan tests; 6 feature-gated operation tests; 17 recovery tests; 3 real AppContainer/Job Object tests; qpdf 12.3.2 bundle verification; single-instance and real WebView2 raw-IPC smokes; the production G03 boundary scan; and the Tauri no-bundle release build.
- This remediation changes only the search scheduler, its unit tests, the focused browser assertion and this log. Physical drag/drop, retained-handle sessions, raw IPC, Tauri/WebView2 construction, PDF.js, CSP, navigation and production window behavior remain byte-identical to the manually accepted build.
