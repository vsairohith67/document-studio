# G04B2 PDF-to-Images Implementation Log

## Scope

G04B2 implements only `pdf.to-images@1.0.0` from accepted main `6940a1381822a5872f4c345cc0e5cd15b2e6294c`. It reuses accepted PDF.js 6.2.108 rendering, `image` 0.25.10 encoding, G03 opaque viewer sessions and G03/G04B durable multi-output publication. It adds no renderer dependency, Tauri capability, app-defined custom protocol, runtime network or system discovery. G04C and G05 remain out of scope.

The operation accepts one unencrypted local PDF, 1-128 unique selected pages in the displayed output order, JPEG/PNG/lossless WebP and exactly 72, 150 or 300 DPI. Output names are reserved before rendering as `<stem>-page-0001.<ext>` with accepted collision suffixing. React receives only display metadata and opaque session/destination identifiers.

## Sequential render and raw IPC boundary

The complete page plan is measured before job creation at viewport scale `DPI / 72`. Non-finite or invalid dimensions fail closed. Each axis is capped at 8,192, each page at 16,777,216 pixels (about 64 MiB raw RGBA) and the entire job at 67,108,864 pixels. The aggregate is the conservative four-maximum-page work budget and applies even though the public page-count limit is 128. Sequential ownership bounds the raw RGBA plus derived RGB payloads for one maximum page to about 112 MiB before bounded codec allocations; no later page begins until the current canvas, transfer and encoded output have been released or persisted.

PDF.js uses the matching local worker, bounded range reads, `isEvalSupported: false`, XFA off, forms off, annotations off, print intent and an explicit opaque-white background. It renders one private canvas, extracts one RGBA buffer, transfers it, releases the page/canvas and only then begins the next page. Browser blob encoders are not used.

The binary Tauri request body is authenticated by headers containing job ID, render-session ID, page ordinal, one-use nonce and expected width/height. Rust verifies exact job ownership, cancellation, strict next ordinal, nonce, dimensions, caps, alpha and `width * height * 4` before consuming the transfer. Stale, replayed, wrong-page, wrong-session, over-cap and payload-size mismatches fail closed. No pixel data is encoded as JSON or base64 and no destination path crosses this boundary. Native WebView2 evidence found that the prior `connect-src 'self'` forced Tauri onto its JSON `postMessage` fallback, so the reviewed CSP now also permits only Tauri's authenticated built-in `ipc:` and `http://ipc.localhost` command origins. This introduces no app-defined protocol, HTTP capability or external-network route.

## Rust encoding and verification

Rust removes the already-proven opaque alpha channel and encodes from RGB with existing `image` 0.25.10:

- PNG: lossless, adaptive filtering, deterministic pHYs resolution metadata.
- JPEG: fixed quality 92, opaque white, JFIF density metadata.
- WebP: lossless only.

Format is selected from the typed contract, never inferred from the extension. Verification reopens each staged file, confirms magic/format, successful bounded decode, nonzero exact dimensions, density where supported, expected page ordinal, byte size and SHA-256. PNG and WebP decoded pixels must exactly match the PDF.js canvas RGB. JPEG must stay within a mean absolute pixel difference of 12. The colour contract is browser/PDF.js canvas-rendered colour; source colour-space preservation is not claimed.

Every expected output row and name reservation exists before rendering. Exact staging-directory membership and every recorded size/SHA-256 are revalidated before the first publication. The no-overwrite publisher also compares its final pre-copy size/hash with that evidence, closing the last-moment change window. Multi-file publication is not atomic. If it stops after publishing a subset, those user files remain published, remaining owned staging is reconciled and history records `PARTIAL_PUBLICATION` truthfully.

## Cancellation and recovery

The UI aborts pending range reads, calls `RenderTask.cancel()`, prevents new pages and requests native cancellation. Rust checks cancellation before/after transfer authentication, encoding, verification and every publication. `getImageData()` is synchronous; the implementation checks immediately before and after the bounded call and does not claim instantaneous interruption. Recovery fails an abandoned nonterminal render job, removes only its owned workspace and preserves any published user output.

The automated cancellation suite covers owned cleanup and phase checks. A separately invoked ignored measurement synchronizes cancellation with the start of a maximum-permitted 8,192 x 2,048 PNG encode; on the Windows reference machine, the debug CPU path returned the cancelled result in 2.170 seconds and published nothing. This is measured diagnostic evidence, not an instantaneous-interruption or hardware guarantee. Final Windows WebView2 acceptance records the actual runtime version through system diagnostics; GPU acceleration is optional and correctness remains CPU/software-rendering compatible.

## Focused evidence

Contract tests pin the exact operation/version, page ordering, formats, DPI allow-list and 128/129 boundary. Native tests cover PNG/JPEG/lossless WebP, 72/150/300 density, malformed/encrypted input, subsets/order, resource and aggregate caps, alpha/payload mismatch, stale/replayed/wrong-page transfer, source immutability, cancellation, collision and restart recovery. Existing lifecycle suites continue to cover injected partial publication and no-delete recovery semantics.

The existing PDF.js Playwright framework now proves two G04B2 paths: sequential authenticated raw transfer with canvas/session release, and a richer local fixture exercising rotated CropBox geometry, an embedded Type 3 glyph, a transparency soft mask, ICCBased image data plus CMYK and Lab painting. PNG/lossless-WebP evidence is exact at the raw and decoded boundaries; JPEG uses the documented tolerance. No repository-owned JBIG2 or JPX fixture is available, so the branch makes no extra codec support claim.

## Local stabilization evidence

On 22 August 2026 the stabilized branch passed repository validation, all 19 negative validator probes, internal-link checking, `git diff --check`, Rust formatting, warning-denied targeted clippy, workspace typecheck, the production frontend build and the Tauri release `--no-bundle` build. Exact local dependency restoration used the hash-locked validation requirements and `npm ci --ignore-scripts`; npm reported zero known vulnerabilities in the installed lock.

The complete Vitest run passed 67 desktop tests plus 20 shared-contract tests. `cargo test --workspace --all-targets --locked` passed 153 Rust tests with two manual performance/measurement tests ignored; the G04B2 maximum-page cancellation measurement also passed when invoked explicitly. Both focused G04B2 Playwright/PDF.js cases passed, as did G03, G04A, G04B and G04B2 boundary verifiers. Native Windows acceptance on WebView2 `151.0.4129.101` proved the raw four-byte RGBA body arrived as a `Uint8Array` through binary Tauri IPC, produced one verified/published PNG, rejected missing authentication, replay and a JSON pixel body, redacted paths, and cleaned the owned test destination/profile.

The release gate requires the repository validator, negative probes, link check, diff check, formatting, targeted clippy, typecheck, contracts, G03/G04A/G04B/G04B2 boundary verifiers, focused browser tests, affected Rust tests, exact-head push and PR CI, then a fresh focused review. The PR may be marked ready only when no blocker, high or medium finding remains. It must not be merged by this implementation task.
