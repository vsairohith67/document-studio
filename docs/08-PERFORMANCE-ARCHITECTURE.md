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
- Cache thumbnails by file fingerprint and renderer version.
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
