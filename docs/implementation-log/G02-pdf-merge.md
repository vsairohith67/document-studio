# G02 Production PDF Merge Implementation Log

Status: G02 Review; implementation and bounded independent-review remediation committed on draft PR #5, with independent acceptance re-review pending
Accepted base: `9bc92dd672fc120fb730a53f13a64abe50470f11`
Implementation branch: `feat/g02-pdf-merge`
Target: Windows x64, Tauri 2, qpdf 12.3.2

## Plan-ready synchronization

- Existing Notion G02 goal updated to PLAN READY with page-only 2–128 semantics and the dependency gate retained.
- Existing Asana G02 task renamed to `G02 — PDF Merge — PLAN READY`, notes updated, and kept incomplete.
- Only the requested Canvs element in room `odUF4mDxzT5PNo-3697W` was changed to `G02 PDF Merge — PLAN READY`.
- No GitHub issue or pull request was created because no existing G02 tracking object was found.

## Pre-dependency implementation slice

- Added the `pdf.merge` `1.0.0` operation manifest alongside `diagnostic.copy` without changing the operation or job schema shapes.
- Expanded the shared create request to a discriminated union: one input for diagnostic copy or 2–128 ordered inputs for PDF Merge.
- Increased file inspection to 128 paths and reports `application/pdf` only when the extension is PDF and PDF magic occurs in the first 1,024 bytes.
- Generalized input hash updates to address a persisted ordinal and added private `inputs` and `temp` workspace directories.
- qpdf dependency diagnostics verify/materialize only the manifest-controlled runtime, launch the fixed production AppContainer, require exact 12.3.2 version output, and fail closed when any boundary is unavailable.
- Added shared TypeScript/JSON and Rust contract fixtures/tests for limits, order and intentional duplicate entries.
- Added reviewed acquisition and bundle-verification scripts; both passed against the acquired and freshly reproduced bundles.
- Recorded the engine, sandbox, verification, recovery, licensing and update decisions in ADR-009.
- Applied the binding amendments: `MANIFEST.json` is the only manifest document; one physical snapshot per persisted ordinal; no production `--deterministic-id`; fixed removal flags and argv order; explicit qpdf exit-code tables; identity-deduplicated bounded preflight; precise unsupported-structure/signature wording; and stable fixed-name AppContainer lifecycle.
- Acquisition validation records qpdf 12.3.2's observed official `--version` output as two fixed lines: the exact version followed by `Run qpdf --copyright to see copyright and license information.` Any other line or version fails closed.

## Dependency gate evidence

- The required inventory found no existing `cosign` command and confirmed that Winget offered Sigstore Cosign 3.0.6.
- The separately approved user-scope Winget command installed Cosign 3.0.6. The verified executable reports Git version `v3.0.6`, commit `f1ad3ee952313be5d74a49d67ba0aa8d0d5e351f`, and Windows AMD64.
- The separately approved acquisition verified the official qpdf checksum bundle for identity `ejb@ql.org` and issuer `https://github.com/login/oauth`, then required the 24,555,583-byte archive SHA-256 `8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202`.
- The reviewed resource tree contains 16 files and 8,574,799 bytes total: one manifest plus 15 controlled runtime, license and provenance files. Runtime files are 8,543,752 bytes; license materials are 18,459 bytes; provenance is 8,234 bytes.
- A second fresh signed acquisition was verified and matched all 16 reviewed files byte-for-byte. Its two exact temporary reproduction directories were then deleted; they contained only disposable dependency copies.
- The verifier hard-codes every reviewed file's relative path, size and SHA-256 in addition to checking the resource manifest, version, source archive, signature identity, profile name, configuration version and zero-capability declaration.
- The complete Apache-2.0 text and both upstream qpdf license pages are retained. The unmodified Microsoft Visual C++ 14.44.35211 runtime files from the signed archive are individually manifest-controlled.
- `qpdf --version` reports exactly `qpdf version 12.3.2` followed by the expected copyright-help line.

## Isolation proof

- The test profile is the fixed `DocumentStudio.PdfEngine.Qpdf.V1.Test` name. Repeated creation derives the same SID; cleanup derives and validates that exact SID and never deletes the production profile.
- `CreateProcessW` receives the executable separately from a correctly quoted argument vector. No shell, PowerShell, batch file or PATH discovery launches qpdf.
- The suspended process token is independently checked as an AppContainer token with the exact derived SID and zero capabilities before it is resumed.
- The private unnamed Job Object is queried back to prove kill-on-close, one active process and a 2 GiB process-memory limit. Wall-clock timeout remains owned by the parent.
- The child environment forwards only `SystemRoot` and `WINDIR`; `LOCALAPPDATA`, `TEMP` and `TMP` point to the private test/job temporary directory. A custom block without private `LOCALAPPDATA` failed at `CreateProcessW` with Windows error 203, so the private value is an explicit Windows launch requirement rather than user-profile forwarding.
- Real tests proved qpdf 12.3.2 launch, allowed-workspace write, denial of an ungranted file, loopback-network denial, enforced one-process rejection, owned Job Object termination within two seconds, and survival of an unrelated process.
- The exact amended production merge argv was executed against two repository-owned PDFs inside the AppContainer. qpdf exited 0, produced a nonempty result, reopened it with `--suppress-recovery --check` exit 0, and returned `--is-encrypted` exit 2.
- There is no unsandboxed fallback. The production worker uses the same verified fixed-profile boundary for preflight, merge, and output verification.

## Production implementation

- The operation registry dispatches `diagnostic.copy` unchanged and `pdf.merge` 1.0.0 for exactly 2–128 persisted ordered inputs.
- Job creation rejects unsafe/nonlocal/non-PDF paths and destination/input aliases. Execution denies source write/delete sharing, rechecks identity/size/time, and creates a distinct `inputs/source-NNNN.pdf` for every ordinal, including repeated and hard-linked inputs.
- Expensive qpdf encryption, strict-structure, and page-count preflight is shared only by source identity and runs across at most four unique sources, also bounded by CPU availability. The final qpdf vector retains exactly one `--file=` per ordinal.
- The production builder exactly matches ADR-009 and contains no `--deterministic-id`. `CreateProcessW` receives the executable and encoded argument vector directly; qpdf never receives an original user path.
- Stdin is closed. Stdout/stderr are continuously drained into 64 KiB in-memory tails and never persisted. The owned process has a 30-minute timeout, 2 GiB memory limit, one-process limit, and cancellation through the retained Job Object only.
- Rust independently verifies the owned staging file, PDF header, nonzero size, SHA-256, strict no-recovery reopen, unencrypted result, and exact ordinal-aware page sum. G01 publication then proves final size/hash equality without overwrite.
- Startup recovery fails every pre-publication merge state without resuming, reconciles only matching publication evidence, cleans only proven-owned artifacts, and preserves neighboring or identity-mismatched files.
- The Precision Paper UI provides native add/drop, ordered rows, pointer and keyboard reorder, remove, safe destination/name, stage/byte/item progress, indeterminate qpdf merge, cancellation boundary, verified result path, failure/interrupted recovery, and precise page-only/signature wording. No viewer, PDF.js, thumbnail, or page-range feature was added.

## Independent review remediation

- Every selected PDF now receives a stable selection-row ID that is independent of its path and position. React keys use that ID, so intentionally repeated paths remain distinct through reorder and removal.
- A post-render focus target restores focus deterministically after keyboard or button reorder. Removal focuses the row that takes the removed position, then the previous row, or Add PDFs when the list becomes empty.
- Active-element regressions cover Alt+ArrowUp/Down, both move buttons, first/middle/last removal, Remove, repeated paths, and final-row removal.
- Merge-order regressions no longer compare marker byte offsets in the serialized PDF. A bounded test-only reader asks bundled qpdf 12.3.2 for semantic page-tree order with `--show-pages`, reads each referenced content stream with `--show-object` and `--filtered-stream-data`, and fails closed on malformed, oversized or ambiguous inspection output.
- Semantic regressions cover reordered documents, an intentionally repeated source, a hard-link alias, and document boundaries with multi-page sources.
- Restart errors now use operation-neutral Document Studio wording. A real `pdf.merge` recovery regression checks that the detail is bounded, sanitized, does not name G01, and makes no automatic-resume promise.
- Third-party notices, the repository manifest, this goal record and this log now reflect the bundled qpdf runtime and existing draft PR #5. Repository validation rejects the exact stale dependency and G02 state claims.
- G02 remains incomplete and unmerged. Independent acceptance re-review is pending and G03 remains blocked. PR #5 and the canonical trackers record the exact remediation head and terminal CI run evidence after both CI events finish.

## Verification evidence

| Gate | Result |
|---|---|
| Repository validator | Passed; 132 feature entries verified |
| Link validator | Passed |
| TypeScript | Passed for desktop and shared contracts |
| Frontend/shared tests | Passed; 21 desktop tests and 12 shared-contract tests (33 total) |
| Frontend production build | Passed |
| Rust formatting and clippy | Passed with warnings denied |
| Rust tests | Passed; 96 default tests, one separately executed ignored performance test, and 3 feature-gated AppContainer/Job Object tests |
| Tauri no-bundle release compile | Passed; retained the known cross-platform `.app` identifier advisory |
| PowerShell acquisition and verification | Passed, including signed fresh acquisition and byte-for-byte reproduction |
| AppContainer and Job Object proof | Passed with real qpdf 12.3.2 and the test-only probe |
| Patch whitespace | Passed |

The current FAST implementation pass did not rerun `npm ci` or pip installation because the prompt prohibited installs and network commands. The already-installed locked environment was used for every validator, frontend, Rust, sandbox, single-instance, and Tauri check. Neither npm nor Cargo lockfile changed.

## Performance evidence

Measured on Windows 11 build 26200, Intel Core i7-11800H (8 cores/16 logical processors), 16,866,865,152 bytes RAM, and a Samsung MZVL21T0HCLR NVMe SSD. p95 is the slowest of five samples unless noted.

| Measurement | Actual |
|---|---:|
| Lazy qpdf manager construction (10 samples) | 0 ms p95 at millisecond resolution |
| Inspect two generated PDFs (10 samples) | 2 ms p95 |
| Pre-merge snapshot/version/strict preflight, ten 10-page PDFs | 1,176 ms p95 |
| End-to-end ten 10-page PDF merge | 1,396 ms p95 |
| End-to-end twenty 50-page PDF merge (1,000 pages, 234,020 source bytes) | 1,454 ms p95 |
| Parent working-set growth | 5,595,136 bytes |
| Highest owned qpdf process peak | 8,511,488 bytes |
| Cancellation acknowledgement | 2 ms p95 |
| Started qpdf cancellation to terminal cleanup | 16 ms p95 |

The small-merge, inspection, memory, and cancellation budgets passed. The 1,000-page corpus was structurally useful but not approximately 1 GiB. Five cold and five warm 1 GiB runs would create more than 10 GiB of avoidable temporary I/O in this pre-stage pass, so that byte-volume benchmark and the 128-input full-preflight budget remain unmeasured. They do not block staging but do block final release acceptance until run on the clean reference setup.

## Final scope and security audit

- No migration, schema file, Tauri capability file, npm dependency/lock, Cargo package/lock, cloud, telemetry, viewer, PDF.js, thumbnail, page-range, or G03 feature was added. Draft PR #5 remains in review and unmerged.
- Production source contains no shell or PATH-based qpdf launch and no `--deterministic-id`. Test-only `Command` usage is limited to fixture/probe setup.
- The native window launched successfully against an isolated test-runtime profile and was visually reviewed at 1280×800. The existing ordinary development profile contains a pre-G01 migration checksum ledger that the unchanged G01 fail-closed database policy rejects; it was inspected read-only and not modified. This is local development-data cleanup/upgrade follow-up, not a G02 schema change.
