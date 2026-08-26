# ADR-016: Conservative object-aware balanced PDF compression

Status: Accepted for G04C2B implementation; owner merge pending

## Decision

`pdf.compress-balanced@1.0.0` uses one fixed `balanced-v1` profile. Rust reads the exact immutable snapshot through qpdf 12.3.2 JSON v2, performs bounded page/Form resource traversal, and creates one minimal qpdf partial-JSON update containing only approved indirect RGB8 image XObjects. The update preserves stream and object-stream policy, preserves unreferenced objects, and uses a deterministic document ID. Rust does not reconstruct the PDF.

Only 8-bit `DeviceRGB` DCT streams or simple Flate streams without decode parameters are eligible. Images with masks, decode arrays, alternate/external streams, unsupported filters or colour spaces, unsafe/shared ancestry, small dimensions, or ambiguous graph use remain unchanged and are counted under a closed skip reason. Quality is fixed at JPEG 82 with no resampling.

After strict structural verification, the existing local PDF.js 6.2.108 renderer compares every affected source/candidate page at scale 2 (144 DPI), opaque white, with at most four aggregate native range reads. The reviewed Rust implementation computes nonlinear BT.709 luma SSIM, RGB PSNR and the exact changed-pixel rule. Publication requires all page gates plus both 5% and 65,536-byte savings. Otherwise the accepted durable no-benefit transaction completes successfully with zero outputs.

The qpdf rewrite necessarily creates or refreshes the PDF trailer `/ID`. `--deterministic-id` makes this boilerplate reproducible, and structural comparison excludes only that trailer field and stream `/Length`; all selected image dictionaries are compared with their filter/data placeholders and all protected objects and streams remain in the semantic inventory.

## Rejected alternatives

- Whole-document qpdf image optimisation cannot prove the selected-object allow-list.
- A new PDF parser or qpdf native ABI would add a separate dependency and attack surface.
- Page rasterisation would destroy searchable text, vectors, forms and document structure.
- A quality slider would make the v1 evidence and output contract non-deterministic.
- Publishing a below-threshold candidate would contradict the accepted G04C2A no-benefit contract.

## Consequences

The operation is intentionally conservative. Correct inputs may produce no file, including the committed six-photo corpus when every quality-82 candidate is larger than its frozen source JPEG. No network is used at runtime or in CI. Signed, encrypted, recovered, malformed, over-budget, stale-render or visually failing documents are refused before publication. G04C2B remains unmerged until its exact-head CI and independent review pass and the owner performs the separate merge gate.
