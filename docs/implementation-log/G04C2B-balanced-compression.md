# G04C2B conservative balanced PDF compression

## Delivery boundary

This slice starts from accepted corpus merge `94c5bb583b7730f26635229be592cd4eeeab6a07` and implements only `pdf.compress-balanced@1.0.0`. It does not add strong compression, OCR, repair, linearisation, Office conversion, G04E1, G04F1 or G05. This branch and its pull request require an owner merge gate and must not be merged by the implementation run.

## Fixed operation

- Exactly one local, unencrypted, signature-free PDF and zero or one output.
- The only settings payload is `{ "profile": "balanced-v1" }`; the durable canonical spec also fixes JPEG quality 82, no resampling, PDF.js 6.2.108, scale 2, opaque white, and the two publication thresholds.
- qpdf 12.3.2 runs through the accepted zero-capability AppContainer and Job Object. It receives only private workspace-relative paths.
- Bounded raw page/Form resource traversal selects only safe indirect RGB8 DCT or simple-Flate image XObjects. One partial JSON update is applied once with preservation flags and a deterministic document ID.
- Strict qpdf, page-count, encryption, signature, source-immutability, selected-object and protected semantic inventories precede visual work.
- Every affected page is rendered source then candidate at 144 DPI through opaque sessions and raw IPC. One shared limiter permits at most four native range reads across both documents.
- Rust applies the frozen SSIM, PSNR and changed-pixel definitions. A stale, duplicate, wrong-side, wrong-ordinal, wrong-generation, wrong-size or non-opaque upload fails closed.
- Publication requires at least 65,536 saved bytes and at least 5%. The existing no-overwrite commit boundary is reused only after all structural and visual gates. A below-threshold result calls the accepted G04C2A transaction and creates no `job_outputs` row or file.

## Durable evidence and privacy

Migration 7 adds strict scalar audit and closed skip-count tables keyed to a balanced job. SQLite stores counts, thresholds, hashes and timestamps only—never qpdf JSON, stream bytes, page pixels, document text or a new document path. Render pixels are sequential and cleared after each authenticated upload, including rejected uploads. Viewer metadata contains opaque session IDs, not source/candidate paths. Balanced job responses redact source, destination, staging and partial paths; only an actually published final path is returned to the owning UI.

The six-photo `CORPUS_MODE=reviewed-rebaseline` corpus is the authoritative offline photographic evidence. All six current frozen JPEGs decode and retain their reviewed provenance; under fixed quality 82 each generated candidate is larger, so the aggregate fixture truthfully completes `no-benefit` with six `candidate-not-smaller` skips and no visual event or output. A deterministic generated RGB fixture separately proves selected-object replacement, byte-identical reruns, structural preservation, the visual/publication sequence and both savings thresholds without substituting another photograph into the corpus.

## Safety and recovery

Cancellation remains available through candidate copy and becomes too late only at the accepted publication commit CAS. Errors close both viewer sessions, clean only marker-owned workspace data and end failed/cancelled when cleanup is proven; cleanup ambiguity or publication-state ambiguity remains interrupted for startup reconciliation. Recovery never resumes qpdf, encoding or rendering and never deletes an accepted published user file.

## Verification status

Focused Rust metric, graph, allow-list, corpus no-benefit, deterministic qpdf update, durable audit, cancellation, encryption/signature/source-change refusal, stale-upload cleanup, spec-tamper and beneficial-publication tests are part of the slice. Frontend tests cover the fixed-profile UI, accessibility, sequential opaque rendering, geometry refusal and the shared native-read bound.

Local pre-PR evidence on 2026-08-26 passed:

- locked Python validation dependency installation and `npm ci --ignore-scripts` with zero reported npm vulnerabilities;
- repository validator, internal link checker, offline corpus validator, every G03-G04 boundary verifier and `git diff --check`;
- TypeScript typecheck, 105 desktop tests, 29 shared-contract tests and the production Vite build;
- `cargo fmt --check`, strict Clippy with `-D warnings`, and all 200 executed Rust tests after repair cycle 1; the two existing manual performance probes remained ignored by the normal suite;
- all 22 real-Chromium PDF.js/browser cases;
- the native WebView2 smoke on runtime 151.0.4129.107, including raw IPC, replay rejection, path redaction and cleanup regression evidence;
- production WebView2 inherited-argument and single-instance smokes; and
- the Tauri optimized no-bundle build, producing `target/release/document-studio.exe`.

Exact-head CI and independent-review results remain to be recorded in the pull request before the owner gate; no unrun result is claimed here.

## Independent review repair cycle 1

The first exact-head review of `34ed716aff524c826c09dd41b2c8c935ef86840c` returned two HIGH and three MEDIUM findings. All five were addressed before requesting a replacement exact-head gate:

- proven balanced publications now recover through the same atomic `completionKind=published` transaction used by the live path;
- structural verification binds every object to its exact object identifier and retains indirect-reference topology while normalising only the selected image stream fields that the fixed profile may change;
- DCT frame width, height, pixel count, precision and three-component identity are checked from the JPEG header before full decode, and decoded images must remain native RGB8 rather than being converted;
- candidate visual dimensions must exactly equal the authenticated source upload dimensions at the Rust boundary; and
- cancellation/time-budget checkpoints run before and between source decode, quality-82 encode, candidate decode and metric computation.

Focused regressions cover balanced crash recovery, reference swaps, object-content swaps, oversized/mismatched DCT headers, grayscale DCT data under an RGB dictionary, equal-byte-count geometry swaps and cancellation immediately after source decode. The superseded first CI run was cancelled after the findings were confirmed; it is not final acceptance evidence.

Repair-cycle re-review found one remaining MEDIUM scoped-resource collision: different nested Forms may legally reuse `/Im0` on the same page. Cycle 2 removed page/name masking and binds the allowed mutation set directly to the exact indirect image object reference selected for the qpdf patch. Its regression models two Forms with separate `/Im0` objects, proves the selected object may change, and proves the same-named unselected object may not. The superseded second CI run was cancelled and is not final acceptance evidence.

## Final frontend lifecycle remediation

The exact-head review of `0919df65ed0178650a9d4f1d4748a2e7486e27a2` identified two remaining frontend lifecycle defects. A balanced job could outlive `OptimizeWorkspace` when unmount occurred before visual rendering, and a supplemental audit-read failure could enter the fatal visual-error path after durable terminal success.

The remediation gives each balanced frontend operation one generation-bound owner that records native-create state, dispose intent, its returned job ID, the latest sequenced state and one deduplicated reconciliation request. Unmount intent now survives a pending native create; known nonterminal jobs are cancelled and reloaded exactly once; visual-ready and other stale callbacks are rejected after disposal; active render abort uses the existing `RenderTask` cancellation path; and known publishing, published and no-benefit states are preserved. A cancellation that loses the publication race reloads the authoritative job and never deletes the accepted output.

Terminal `JobRecord` display and busy-state completion now happen before a separately bound, non-fatal audit read. An audit failure cannot cancel the job, change `completionKind`, hide published actions, invent output for no-benefit or expose the underlying IPC/database error. Audit responses are accepted only for the exact current job and generation.

The first focused re-review of remediation head `eb7fbc71bf1776f5b5269ff59464554f7b07e4cf` found a HIGH active-upload cancellation race and a MEDIUM replacement-operation identity race. The follow-up repair adds a native post-lock cancellation checkpoint and bounded CPU-copy checkpoints, permits authoritative status refresh after an initially nonterminal cancellation snapshot without duplicating the cancellation request, retires the previous job identity before a replacement create starts, and keys buffered visual-ready events by exact job ID and generation. This narrowly necessary native lifecycle change does not alter the G04C2 contract, profile, metrics, qpdf policy, corpus, migration, dependencies or publication thresholds.

The focused lifecycle/UI matrix passes 31 tests, including 32 deterministic held-create/unmount races, 32 deterministic pre-visual/unmount races, all three enabled rail exits, active render abort, publication-too-late preservation, remount-and-run, replacement-operation stale-event rejection, direct and progress-path audit failures, delayed old audits and unmount during audit. The local regression pass also passes 130 desktop tests, 29 contract tests, 201 executed Rust tests with the two existing manual performance probes ignored, 22 real-Chromium cases, nine PDF.js asset cases, repository and link validation, the G04C2 corpus substitution probes, the G04C2B boundary verifier, strict Clippy, the production frontend build and the optimized Tauri no-bundle build. The browser and PDF.js suites were run once because the follow-up repair changes lifecycle ownership and native cancellation checkpoints, not route rendering or vendored assets. Fresh repaired-head CI and focused independent re-review remain required before the owner merge gate; PR #17 remains open and unmerged.

## Bounded performance evidence

The reference machine was Microsoft Windows 11 Home Single Language build 26200, Intel Core i7-11800H (8 cores/16 logical processors), 42,636,668,928 bytes visible RAM and Samsung MZVL21T0HCLR-00B00 NVMe. The local toolchain was Rust 1.97.1, qpdf 12.3.2, project Chromium 151.0.7922.34 and WebView2 151.0.4129.107.

The committed six-page/six-image corpus PDF is 2,200,894 bytes and embeds 2,197,866 JPEG bytes. Its fixed-profile run selected zero replacements, recorded six `candidate-not-smaller` skips, retained candidate/audit size 2,200,894 bytes and completed no-benefit with zero output. One isolated debug test sample, excluding compilation, completed this end-to-end backend path in 7.48 seconds. The deterministic two-run qpdf candidate/structure comparison took 4.60 seconds; the 2,048 x 2,048 synthetic beneficial backend path, including authenticated scalar visual submissions and durable publication but excluding browser rendering, took 20.45 seconds; preflight cancellation and cleanup took 0.12 seconds. These are focused single samples, not cold/warm percentile claims.

The real browser regression recorded a peak of one native read in its 100- and 1,000-page viewer probes, and the native WebView2 smoke recorded five session/range/close samples. Those are inherited renderer/transport evidence, not a claim of balanced end-to-end render performance. Peak parent/qpdf/WebView memory, balanced cold/warm end-to-end percentiles and stage-separated decode/encode/qpdf/render/metric/publication timings were not instrumented in this run. No target is claimed for those unmeasured values, and no safety or quality threshold was weakened.
