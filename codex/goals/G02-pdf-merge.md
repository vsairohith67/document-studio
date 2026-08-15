# G02 — PDF Merge Vertical Slice

Implement PDF Merge as the first production operation.

Outcome: add and inspect multiple PDFs, drag reorder, optional page ranges/bookmark policy, output naming and destination, truthful preflight/progress/cancel, qpdf execution, verified atomic output, history and continue-with-result.

Constraints: use the shared operation contract and job engine; checksum-govern qpdf; validated argument arrays only; never rasterize structural merges; never overwrite inputs; no silent cloud fallback; pause for ADR/dependency changes.

Verification: openability, expected page count and source order, encryption state, corrupt/encrypted input errors, cancel leaves no final partial, naming collision behavior, crash recovery, golden corpus, performance baseline, UI keyboard/screenshot review and updated docs.
