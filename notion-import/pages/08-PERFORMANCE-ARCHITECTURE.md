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
