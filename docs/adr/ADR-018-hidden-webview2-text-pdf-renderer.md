# ADR-018: Hidden intercepted WebView2 TXT-to-PDF renderer

Status: Accepted for G04E1 implementation; owner merge pending

## Decision

`text.to-pdf@1.0.0` uses a dedicated hidden Rust-owned WebView2 controller on
an STA/message-pump thread. Rust supplies the complete escaped HTML document,
fixed CSS, and three approved static Noto Regular fonts from immutable memory
responses at the exact internal origin
`https://txt-renderer.document-studio.invalid/1/`. A global resource filter is
installed before navigation for every context and request-source kind. Every
request receives an application-generated response; unexpected requests fail
closed and never pass through to a network, loopback server, `file://`, custom
protocol, system font, or alternate renderer.

The Windows-only desktop manifest directly declares exactly:

```toml
webview2-com = "=0.38.2"
windows = { version = "=0.61.3", default-features = false, features = [
  "Win32_System_Com",
  "Win32_UI_Shell",
] }
```

`webview2-com` provides the generated WebView2 environment/controller,
resource-request, settings, denial, and `PrintToPdf` interfaces. Although the
same crate is already locked transitively through Tauri/Wry, Cargo does not
permit Document Studio to name a transitive crate and Tauri does not re-export
the required `ICoreWebView2_22`, `ICoreWebView2Environment6`, or
`ICoreWebView2_7` projections. The direct edge therefore records a real
production interface dependency without changing version `0.38.2`.

The `windows` projection supplies only the typed `IStream` and
`SHCreateMemStream` path needed by `CreateWebResourceResponse`. Response sizes
are checked against `u32::MAX`, immutable bytes and COM response resources are
retained for the response lifecycle, and `IStream::Stat` must equal the served
length. Generated projection ownership and `Result`/HRESULT handling are used;
manual `QueryInterface`, vtables, `AddRef`/`Release`, `transmute`, and unchecked
pointer ownership are prohibited. `windows-sys` remains appropriate for the
existing file/process boundary but cannot replace typed COM ownership.

No direct `windows-core` edge is needed: both approved crates resolve the
already locked compatible `windows-core 0.61.2`, and the `windows 0.61.3`
public projection exposes the required core types. A future compile failure
that genuinely requires a new direct edge is a separate owner gate.

WebView2 remains the platform-provided Evergreen runtime. Document Studio
bundles no WebView2 runtime and performs no runtime download or renderer
discovery. The accepted SEC1C environment scrub executes before any WebView2
environment. Each text job uses a unique marker-owned UDF and never accesses,
redirects, or deletes the primary persistent application profile.

Raw WebView2 output is private. qpdf 12.3.2 rebuilds the page-only normalized
candidate in the accepted zero-capability AppContainer/Job Object, after which
Rust independently verifies paper boxes, rotation, forbidden actions/content,
metadata/path leakage, exact embedded font inventory, source immutability, and
final publication byte equality.

## Provenance and update policy

- `webview2-com 0.38.2`: crates.io checksum
  `7130243a7a5b33c54a444e54842e6a9e133de08b5ad7b5861cd8ed9a6a5bc96a`,
  MIT, upstream `wravery/webview2-rs`.
- `windows 0.61.3`: crates.io checksum
  `9babd3a767a4c1aef6900409f85f5d53ce2544ccdfaa86dad48c91782c6d6893`,
  MIT OR Apache-2.0, upstream Microsoft `windows-rs`.

Both versions were already present in the accepted lock. Updates require
explicit owner approval, checksum/licence/advisory review, compile-surface
proof, hostile-environment and no-network tests, real WebView2 rendering,
output security verification, exact-head CI, and independent review.

## Consequences

The operation is Windows-only and same-runtime rendering is the reproducible
evidence boundary; byte-identical PDF output across Evergreen runtime versions
is not promised. G04E1 adds no database migration, Tauri capability, network
dependency, HTTP server, Office engine, Markdown/HTML input, batch execution,
system font installation, packaging, or deployment.
