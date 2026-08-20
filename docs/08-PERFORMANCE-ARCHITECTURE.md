# Performance Architecture

## Performance objective

The product should feel immediate for common local tasks and explain unavoidable delays for OCR, Office conversion and AI. Performance is measured on a named reference machine and with published test documents.

## Reference budgets (initial targets)

| Interaction | Target |
|---|---|
| Warm desktop launch to usable Home | <= 1.5 s |
| Cold desktop launch | <= 3.0 s |
| First visible page of a normal PDF | <= 1.0 s after selection |
| 100-page thumbnail strip usable | <= 3.0 s with progressive rendering |
| Merge ten 10-page text PDFs locally | <= 2.0 s excluding antivirus overhead |
| Rotate/reorder metadata operation | <= 1.5 s |
| Cancel acknowledgement | <= 250 ms; worker termination/cleanup reported separately |
| UI input response while jobs run | <= 100 ms |
| Job progress update | 4-10 Hz, throttled to avoid UI overhead |

These are engineering targets, not guarantees. They must be calibrated on Windows hardware used by the owner.

## Techniques

- Keep structural operations in-process or in a warm trusted sidecar/CLI adapter.
- Avoid copying large inputs unless the operation or safety model requires it.
- Memory-map/read streams where supported; never load a multi-gigabyte file into the UI process.
- Render first page and visible thumbnails first; virtualize the rest.
- Cache thumbnails only in bounded session memory unless a later privacy review approves persistence; G03 persists none.
- Separate CPU, I/O and memory-heavy queues with concurrency limits.
- Keep LibreOffice and OCR warm only when resource budgets allow.
- Use libvips for low-memory streaming image transforms.
- Publish outputs with atomic rename after verification.
- Benchmark every operation with 10, 100, 500 and 1,000-page fixtures where practical.

## Fast-path router

1. Inspect file, size, encryption, page count and operation.
2. Choose browser-local, desktop-local or cloud worker according to capability and policy.
3. Estimate work units and resource risk.
4. Run with bounded concurrency.
5. Verify output and record timing by stage.
6. Feed benchmarks into regression thresholds.

## G02 measured baseline — 17 August 2026

Reference machine: Windows 11 build 26200; Intel Core i7-11800H (8 cores/16 logical processors); 16,866,865,152 bytes RAM; Samsung MZVL21T0HCLR NVMe SSD; qpdf 12.3.2. Each reported p95 is the slowest of five measured samples unless stated otherwise.

| G02 measurement | Result | Budget result |
|---|---:|---|
| Lazy qpdf manager construction, 10 samples | 0 ms p95 at millisecond resolution | Within 100 ms startup-overhead budget |
| Inspect two generated PDFs, 10 samples | 2 ms p95 | Within 250 ms |
| Snapshot/version/strict preflight before merge for ten 10-page PDFs | 1,176 ms p95 | The two-input and 128-input plan budgets were not measured by this corpus |
| End-to-end merge of ten 10-page generated PDFs | 1,396 ms p95 | Within 2 s |
| End-to-end merge of twenty 50-page PDFs (1,000 pages, 234,020 source bytes) | 1,454 ms p95 | Page-load evidence only; not the planned approximately 1 GiB byte-volume case |
| Parent working-set growth during the bounded run | 5,595,136 bytes | Within 150 MiB |
| Highest observed owned qpdf process memory | 8,511,488 bytes | Within 512 MiB target and 2 GiB hard limit |
| Cancellation API acknowledgement | 2 ms p95 | Within 250 ms |
| Started qpdf cancellation to terminal cleanup | 16 ms p95 | Within 2 s termination and 5 s ordinary cleanup targets |

The generated 1,000-page corpus is structurally representative but intentionally tiny. Creating and processing the planned approximately 1 GiB corpus five cold and five warm times was not practical inside this bounded pre-stage run because it would cause over 10 GiB of avoidable temporary I/O. This does not block staging; it remains a blocking clean-machine release-acceptance benchmark. The 128-input full-preflight budget also remains a pre-release measurement. No unmeasured number is presented as achieved.

## G03 measured baseline — 17 August 2026

Reference machine: Windows 11 Home build 26200; Intel Core i7-11800H (8 cores/16 logical processors); 16,866,865,152 bytes RAM; Samsung MZVL21T0HCLR NVMe SSD. Browser evidence used project-local Chrome for Testing 151.0.7922.34; real desktop IPC used WebView2/Edge 151.0.0.0. Renderer was PDF.js 6.2.108 and operations used qpdf 12.3.2. Browser p95 is the slowest of five samples.

Targets for G03 are acknowledgement <=100 ms, first page <=1 s, first thumbnail <=1.5 s, newly requested visible page <=300 ms, zoom response <=250 ms, search first result <=250 ms, mean scripted scroll frame <=20 ms, close/resource release <=250 ms, no more than 8 full canvases/20 thumbnails, positive JS heap growth <=150 MiB on the synthetic 1,000-page case, operation preflight <=1.5 s for the named small fixture, and cancellation acknowledgement/cleanup <=250 ms when cancelled before processing. These are named engineering targets, not universal document guarantees.

| Browser viewer measurement | 100 pages cold median / p95 | 100 pages warm median / p95 | 1,000 pages cold median / p95 | 1,000 pages warm median / p95 |
|---|---:|---:|---:|---:|
| Action acknowledgement | 0.3 / 0.5 ms | 0.2 / 0.2 ms | 0.2 / 0.3 ms | 0.1 / 0.3 ms |
| Test transport session response | 5.5 / 11.6 ms | 5.0 / 6.1 ms | 5.8 / 6.4 ms | 5.0 / 5.1 ms |
| PDF document ready | 213.2 / 232.9 ms | 217.8 / 241.1 ms | 254.9 / 257.3 ms | 253.3 / 263.9 ms |
| First visible page | 296.3 / 332.0 ms | 291.5 / 335.6 ms | 340.4 / 349.0 ms | 326.4 / 345.2 ms |
| First thumbnail | 338.2 / 373.4 ms | 325.8 / 374.5 ms | 379.3 / 389.9 ms | 358.1 / 372.1 ms |
| Close and zero mounted canvases | 72 / 78 ms | 54 / 72 ms | 67 / 92 ms | 90 / 97 ms |
| Peak positive Chromium JS heap growth | 11,692,960 bytes | 8,753,704 bytes | 12,337,796 bytes | 8,367,380 bytes |

The deterministic 100-page fixture is 39,169 bytes and the 1,000-page mixed-size/orientation fixture is 393,983 bytes. Both are structurally useful but much smaller than image-heavy user documents. Each open retained at most 3 full-resolution canvases and 10 thumbnails; close retained zero. The 1,000-page interaction p95s were 190 ms to render the requested last page, 220 ms for zoom, 125 ms to first search result and 17.45 ms mean per scripted scroll frame. No result claims behavior for a large scanned/image PDF that was not measured.

Real WebView2 raw Tauri IPC used the 44-page, 1,079,368-byte repository blueprint. Five sequential samples produced session-open median 1.3 ms/p95 374.4 ms (first cold Tauri invocation), one 256 KiB positioned read median 145.4 ms/p95 157.8 ms, and close median 1.1 ms/p95 2.1 ms. The bridge returned bytes, not base64; a closed generation was rejected. This smoke measures the native transport, not full PDF.js rendering.

A separate native-window inspection used the backend Ctrl+O dialog to open the same 44-page blueprint. The production WebView2 path rendered progressive pages and thumbnails with selectable text, exposed only the display filename in the UI, and returned to the empty workspace after Close. Physical cross-window file drop was not automated because the available GUI driver confines drag coordinates to its source window; Rust drag-event behavior remains covered by compiled handler and session-boundary tests and is retained as a manual pre-PR check.

Five four-page/1,146-byte adversarial qpdf operation samples measured complete durable processing, verification, publication and cleanup: extract 417.93/424.51 ms median/p95, remove 386.29/404.60 ms, reorder 394.36/420.62 ms, rotate 774.47/990.12 ms, and three-output split 810.86/819.70 ms. A separate 1,137-byte fixture measured preflight 178.32/198.80 ms. Pre-execution cancellation and cleanup measured 13.39/14.14 ms. These small structural fixtures meet their named targets but do not establish throughput for large/image-heavy PDFs or a 128-output split; those remain non-blocking benchmark follow-ups before release claims.
