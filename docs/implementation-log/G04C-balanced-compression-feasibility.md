# G04C Balanced PDF Compression Feasibility

## Status and decision

**Planning only — not active implementation.** No G04C branch, dependency, migration, operation registration or production source change is authorized by this note.

The accepted stack can perform a limited whole-document image optimization without rasterizing pages: qpdf 12.3.2 can rewrite eligible image streams as JPEG and can recompress Flate streams. It does not resample images, and `pdf-writer` 0.15.0 is a low-level creator rather than a parser/mutator. The qpdf command-line optimizer also does not expose an object allow-list. Therefore the stack is sufficient for a conservative feasibility spike, but it is **not yet sufficient evidence for the product operation** described below. G04C must first prove eligibility, preservation and visual-quality rules against a representative corpus or approve a bounded qpdf object-API integration.

Primary references:

- [qpdf image optimization and thresholds](https://qpdf.readthedocs.io/en/stable/cli.html#option-optimize-images)
- [qpdf file-size optimization limits](https://qpdf.readthedocs.io/en/stable/cli.html#optimizing-file-size)
- [qpdf stream decode and recompression behavior](https://qpdf.readthedocs.io/en/stable/cli.html#option-decode-level)
- [pdf-writer 0.15.0 low-level validation note](https://docs.rs/pdf-writer/0.15.0/pdf_writer/#note)
- [image 0.25.10 decoder resource limits](https://docs.rs/image/0.25.10/image/struct.Limits.html)

## Feasibility questions

### 1. Can the accepted stack recompress embedded images without rasterizing pages?

Partly. qpdf 12.3.2 `--optimize-images` operates on image objects rather than rendered pages. It can convert non-DCT images to JPEG when the candidate is smaller; `--jpeg-quality` also permits recompressing existing JPEG images. It does not resample images. This preserves page content as PDF objects, but it cannot express Document Studio's proposed per-object safe/skip policy from the command line.

`pdf-writer` cannot safely fill that gap because it does not parse an existing object graph. The `image` crate can decode and encode pixels but does not understand PDF color spaces, image masks, shared objects or page resources.

### 2. Can it preserve the rest of the document?

The intended operation changes image stream data only. Searchable text, text positioning, links, forms, annotations, outlines, page labels and non-image page content should remain logically unchanged when qpdf rewrites the document. Transparency and ICC behavior depend on the image dictionary, masks and color space, so they need explicit eligibility rules rather than a blanket preservation claim.

Acceptance must prove all of the following, not infer them from a structurally valid output:

| Feature | Required evidence |
| --- | --- |
| Searchable text and positioning | same normalized PDF.js text items, strings, page indices and bounded position tolerance |
| Links | same link targets and page association |
| Forms | same AcroForm fields, values, flags and widget count; no appearance regeneration |
| Annotations | same subtype/count/rect/action inventory |
| Outlines and page labels | same normalized hierarchy, destinations and labels |
| Transparency | unchanged non-image graphics plus exact mask association for any supported image |
| ICC profiles | exact preservation or explicit skip; never silent DeviceRGB normalization |
| Non-image content | object/content inventory and PDF.js representative-page comparison |

### 3. Which image objects are safe candidates?

For a first production slice, only indirect `/Subtype /Image` XObjects that satisfy every declared rule:

- bounded width, height, decoded pixels and decoded bytes;
- 8-bit `DeviceGray` or `DeviceRGB` color;
- supported Flate or DCT data and a dictionary qpdf can decode without warning;
- no image-mask semantics, color-key mask, soft mask, alternates, unusual `/Decode`, OPI, JPX metadata or external data source;
- stable indirect object identity and a complete reference inventory;
- a candidate output that is smaller and passes the visual threshold.

This conservative list intentionally favors photographs and ordinary scans. The allow-list must be executable policy, not prose only.

### 4. Which objects must be skipped or block the first slice?

Skip inline images and small images. Fail the whole first-slice eligibility check, or return a truthful no-change result, for `/ImageMask`, `/Mask`, `/SMask`, ICCBased, CalGray/CalRGB/Lab, Indexed, Separation, DeviceN, JPX, JBIG2, CCITT, multi-filter chains not proven by fixtures, non-8-bit samples, malformed dictionaries, decode warnings, oversized decoded data or ambiguous shared references.

Signed and encrypted documents are separate fail-closed cases. A later version may add narrowly tested categories without changing the versioned behavior of `1.0.0`.

### 5. How should JPEG and lossless streams be treated?

- Existing DCT/JPEG: a balanced profile may recompress at fixed quality only when the candidate is smaller and passes visual comparison. Record that this is generationally lossy. Never repeatedly compress an output without an explicit new job and warning.
- Lossless photographic Flate: may become JPEG only under the safe candidate rules and quality gate.
- Line art, bilevel images, screenshots, diagrams and text-heavy scans: skip in v1 because JPEG ringing can be visible even when a global metric looks acceptable.
- Existing Flate non-image streams: lossless recompression may be retained as the already accepted G04A behavior, but its byte savings must be reported separately from image savings.

### 6. How are masks and soft masks handled?

V1 skips any image with `/Mask`, `/SMask` or `/ImageMask`. This avoids color-key changes, alpha-edge halos, dimension mismatch and accidental recompression of the mask as a photograph. A later mask slice must treat the base image and mask as one atomic candidate, preserve dimensions and references, use a lossless mask stream and compare alpha edges against a transparent-background corpus.

### 7. How are shared XObjects handled?

Inventory by indirect object number and generation, not by page/resource name. A shared image is decoded and evaluated once, replaced once and continues to be referenced by every original user. If the engine cannot prove a single atomic replacement without cloning or dropping a reference, skip it. Tests must cover one image reused across pages and nested form XObjects.

### 8. How are digital signatures reported?

Any signature field, signature dictionary or signature byte-range is a hard preflight refusal for v1. Rewriting a PDF changes bytes and can invalidate signatures; the UI must say that the signed document was not modified. Do not publish an output, do not describe a visual signature appearance as a valid signature, and do not offer an override in the balanced profile.

### 9. What visual metric and corpus are practical?

Use the accepted PDF.js renderer at a pinned version and deterministic Windows/browser configuration. Render all pages for small fixtures and a deterministic representative set for large documents at 144 DPI. Compare a fixed sRGB RGBA buffer with:

- structural similarity (SSIM) at least `0.985` per compared page;
- peak signal-to-noise ratio (PSNR) at least `36 dB` per compared page;
- maximum changed-pixel ratio above delta 12 no greater than `0.5%`;
- alpha mismatch exactly zero in any later mask-support slice.

Metrics are gates, not substitutes for corpus review. The corpus needs photographs, grayscale scans, text-heavy scans, line art, screenshots, transparency, ICC, rotated/cropped pages, shared XObjects, nested forms, forms, annotations, links, outlines, page labels, signatures, encryption, damaged files and resource bombs. Golden images and expected skip/fail decisions must be reviewed and pinned.

### 10. What does “Balanced” mean deterministically?

Proposed fixed profile `balanced-v1`:

- JPEG quality `82`;
- no resampling and no page rasterization;
- minimum image width `256`, height `256`, area `65,536` pixels;
- inline images preserved;
- object streams preserved;
- generalized streams compressed; Flate recompressed at level `9`;
- only the v1 image allow-list;
- visual gates from question 9;
- no metadata removal, flattening, decryption, repair, linearization or document-feature editing.

The operation spec persists these exact settings and a SHA-256. A later setting change requires a new operation version or named profile.

### 11. When is reduction a success?

Publish a balanced output only when it saves at least both `5%` of the verified source size and `65,536` bytes. Otherwise finish with a typed `NO_BENEFIT` result and no published output. Report source bytes, candidate bytes, saved bytes, saved percentage, eligible image count, changed image count and skipped image count. Never call an output successful merely because qpdf exited successfully.

### 12. What must fail closed or be skipped?

Fail closed on signatures, encryption, malformed/recovered input, qpdf warnings, resource-limit overflow, unsupported object graphs, source mutation, settings/checksum mismatch, visual-gate failure, changed page count, changed protected-feature inventory, unexpected non-image mutation, verification failure, output collision/publication failure or cancellation. Skip only categories explicitly allowed to be skipped by the versioned contract; if an unsafe category could still be changed by the chosen engine, the entire job is ineligible.

### 13. What tests are required?

See the matrix below. Minimum acceptance includes exact-argument sandbox tests, object eligibility fixtures, source/output semantic inventories, visual corpus gates, no-benefit behavior, signatures/encryption refusal, shared/nested XObjects, cancellation at every stage, publication/recovery, database migration/constraints, UI accessibility and a real WebView2 run.

### 14. Is this a reasonably small milestone?

Not as one implementation milestone. A qpdf-only command is small, but the evidence needed to call it balanced and preservation-safe is not. Object-aware selection and verification are the real work. Treating the command invocation as the feature would create an unverifiable product claim.

### 15. Should G04C be split further?

Yes:

1. **G04C1 — feasibility and corpus gate:** no operation registration; freeze corpus, implement a non-product spike/harness, validate exact qpdf 12.3.2 behavior, decide whether qpdf CLI document-level eligibility is sufficient or whether the qpdf object API is required.
2. **G04C2 — conservative balanced v1:** only the proven allow-list, fixed profile, no masks/ICC/inline images, complete audit and no-benefit path.
3. **G04C3 — advanced image classes:** separately approve masks, ICC and any additional color spaces/object types after dedicated evidence.

## Dependency implications

No new dependency is approved by this plan.

- qpdf 12.3.2 is already accepted and packaged, but the CLI cannot target an allow-list of object IDs. A CLI-only design must therefore reject a whole document whenever qpdf could alter an unsafe image category.
- The packaged `qpdf30.dll` may make a future object-aware native integration possible, but headers, API surface, linking/loading, provenance, ABI compatibility, sandboxing and licensing must pass a new dependency/architecture gate before production use.
- `image` 0.25.10 is suitable only after a PDF engine has decoded a safe image object and described its color/mask semantics. Its limits remain mandatory.
- `pdf-writer` 0.15.0 should not be used to reconstruct arbitrary existing PDFs.
- Do not reintroduce libvips as an assumed solution. It does not provide the PDF object parser/mutator or preservation proof.

## Proposed operation contract

```text
operation: pdf.compress-balanced@1.0.0
inputs: exactly one local PDF
outputs: zero or one local PDF
profile: balanced-v1 (fixed; no free-form quality control)
terminal outcomes: COMPLETED | NO_BENEFIT | FAILED | CANCELLED
success publication: only verified candidate meeting size and visual gates
source behavior: read-only; identity, size and SHA-256 rechecked before publish
audit: settings SHA, source/output hashes and sizes, eligible/changed/skipped counts,
       qpdf identity, semantic inventory hashes, visual metrics, warnings and outcome
privacy: local-only, no telemetry/network, no document bytes or raw paths in diagnostics
```

`NO_BENEFIT` is not a successful publication and is not an error. It must leave the destination unchanged and explain that the verified candidate did not meet the fixed savings threshold.

## Security and privacy invariants

- Content sniffing and canonical path/identity checks precede work.
- One bounded source; bounded file bytes, object count, page count, image dimensions, decoded pixels, per-image allocation, aggregate allocation, process time and output bytes.
- qpdf is resolved only from the accepted bundle and runs through the existing sandbox with an argument allow-list and timeout.
- The private workspace, durable state machine, cancellation, recovery and no-overwrite publication protocol remain mandatory.
- Candidate output is never published before strict qpdf, encryption, page-count, semantic-inventory, visual and source-immutability verification.
- Signature/encryption refusal is explicit and audit-safe. Passwords, document bytes, extracted content, raw paths and image pixels never enter telemetry or durable warning text.
- Engine warnings, unsupported categories, partial visual evidence or missing corpus fixtures cannot be downgraded to success.

## Test matrix

| Area | Required cases |
| --- | --- |
| Eligibility | safe DeviceRGB/Gray Flate and DCT; every skipped color/filter/mask class; inline images; malformed dictionaries |
| Resource bounds | zero/one input; maximum bytes/pages/objects/pixels; decompression bomb; timeout; output expansion |
| Preservation | searchable text/positions, links, AcroForm, annotations, outlines, page labels, rotations/crop boxes, metadata, attachments and non-image streams |
| Image graph | shared XObject across pages, nested form XObject, duplicate resource names, orphan object, cyclic/malformed graph |
| Quality | pinned photo/scan/line-art corpus; SSIM/PSNR/pixel thresholds; deterministic rerun; expected skip |
| Security | signed, encrypted, damaged/recovery-needed, hostile paths, source mutation, spec tamper, qpdf replacement/tamper, sanitized errors |
| Outcomes | reduction above threshold, byte/percentage boundary, `NO_BENEFIT`, visual rejection, cancellation at each stage, verifier/publication failure |
| Lifecycle | crash/startup recovery, owned cleanup, collision naming, output reopen/hash, no source mutation |
| UX | keyboard and screen-reader flow, truthful counts/savings, signed/unsupported/no-benefit messages, copy-path behavior |
| Runtime | unit/integration/property tests, qpdf semantic proof, PDF.js visual run, real WebView2 smoke, release/Tauri build |

## Owner decision required before implementation

Choose one architecture only after G04C1 evidence:

- **CLI-only conservative v1:** smallest change, but whole-document ineligibility whenever an unsafe image category is present.
- **Object-aware qpdf integration:** more useful selection, but a new native API/ABI and packaging gate with materially larger security and maintenance scope.

The recommended first action is G04C1: approve the frozen corpus and a non-product qpdf 12.3.2 feasibility harness. Do not create the production branch until its report selects one architecture and shows that the proposed thresholds are achievable.
