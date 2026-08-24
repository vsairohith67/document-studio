# ADR-014: Own the WebView2 startup environment

## Status

Accepted for SEC1C implementation. Repository acceptance still requires the SEC1C owner merge gate.

## Context

WebView2 reads process environment variables while it creates its environment. An inherited runtime folder, release channel, user-data folder, browser argument, debugger control or a future `WEBVIEW2_*` variable can otherwise alter which runtime starts, where browser state is stored or whether a debugging surface is exposed. Filtering only known browser-argument spellings is not a complete startup boundary.

## Decision

The first action in the Rust desktop `run()` function enumerates the actual process environment keys. Before `tauri::generate_context!()`, plugin construction, window construction or any WebView2 environment creation, it removes every key whose name begins with `WEBVIEW2_` using Windows case-insensitive comparison. It also removes every casing of `COREWEBVIEW2_MAX_INSTANCES`, then sets exactly `COREWEBVIEW2_MAX_INSTANCES=20`.

Production does not restore `WEBVIEW2_USER_DATA_FOLDER` and does not accept a benign exception for `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`. Tauri/Wry continues to select the persistent application-owned profile under the application identifier; the policy neither deletes nor resets that profile.

The feature-gated test runtime receives only explicit `DOCUMENT_STUDIO_TEST_*` controls. Its unique user-data directory and optional loopback CDP arguments continue to be passed directly through `WebviewWindowBuilder`; they are not inherited WebView2 environment controls. No development allowance compiles into the release boundary.

## Consequences

- Known runtime-path, channel, debugger, CDP, background and user-data overrides fail closed, as do unknown future `WEBVIEW2_*` controls.
- The single-instance boundary, raw IPC probe and exact-owned test cleanup remain unchanged in authority and scope.
- Production keeps one stable application-owned profile across ordinary launches.
- Tests must cover mixed casing, future names, malicious runtime/UDF/debug input, the exact value `20`, test-runtime UDF/CDP injection and production child-process command lines.

No dependency, capability, CSP, database, PDF operation or user-document contract changes.
