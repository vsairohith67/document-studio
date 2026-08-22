# Codex Prompt 04C — Balanced PDF Compression

Work only after the owner has accepted the G04C1 feasibility report and explicitly authorized a G04C production branch. Read [the feasibility plan](../../docs/implementation-log/G04C-balanced-compression-feasibility.md), accepted publication/recovery ADRs, contracts, tokens and the exact qpdf manifest before changing code.

## Objective

Implement only `pdf.compress-balanced@1.0.0` with the selected and approved architecture. Preserve PDF structure; do not rasterize pages. Use the fixed `balanced-v1` policy and return a typed no-benefit outcome when the verified candidate saves less than both 5% and 65,536 bytes.

## Hard boundaries

- Do not add G04D-G04F capabilities, OCR, repair, signing, encryption changes, metadata removal, flattening, linearization, arbitrary quality controls or online processing.
- Do not treat qpdf exit success as product success.
- Do not modify signed or encrypted documents in v1.
- Do not process an image category outside the approved executable allow-list.
- If a CLI-only architecture was selected, reject the whole document when qpdf could alter an unsafe category.
- Do not reconstruct arbitrary PDFs with `pdf-writer`.

## Required vertical slice

1. Add the versioned operation contract and exact fixed settings envelope.
2. Add a metadata-only migration for balanced audit fields if the accepted schema cannot represent them; never store document bytes, extracted content, pixels, credentials or raw paths.
3. Preflight canonical source identity, signature/encryption state, qpdf identity, object inventory, candidate eligibility and all resource budgets.
4. Use the accepted private workspace, state machine, cancellation, recovery, sandbox and durable no-overwrite publication path.
5. Generate a candidate with the exact approved engine arguments/API.
6. Verify strict structure, encryption state, page count, protected semantic inventory, fixed visual metrics, size threshold, source immutability, output reopen/size/hash and cleanup.
7. Publish only after every gate passes. Return `NO_BENEFIT` without publishing when savings are insufficient.
8. Build a truthful accessible UI that reports source/candidate size, savings, eligible/changed/skipped counts and safe warnings.

## Required evidence

Implement the entire test matrix in the feasibility plan, including each allowed and skipped image class, shared/nested XObjects, signed/encrypted refusal, limits/bombs, settings tamper, semantic preservation, pinned PDF.js visual metrics, threshold boundaries, cancellation, injected failures, recovery, accessible UI and real WebView2 evidence. Run all applicable repository, frontend, Rust, qpdf, browser, WebView2 and Tauri checks and report every skipped check.

Keep the PR draft until exact-head CI and a fresh detached-checkout acceptance review pass. Do not merge without explicit owner authorization.
