# SEC1C WebView2 startup environment policy

Status: REVIEW — not accepted until the SEC1C owner merge gate

Base: `b5901a7e1152bb251825dc777e6b2992362ea942`

Branch: `feat/sec1c-webview2-environment-policy`

## Implemented boundary

- `run()` enforces the policy before Tauri context generation, plugin construction, windows and WebView2 environments.
- The policy enumerates actual process environment keys, removes every case-insensitive `WEBVIEW2_*` name plus every casing of `COREWEBVIEW2_MAX_INSTANCES`, and sets exactly `COREWEBVIEW2_MAX_INSTANCES=20`.
- No inherited runtime path, release channel, user-data folder, browser argument, debugger/CDP control, background control or unknown future `WEBVIEW2_*` value is retained.
- Production leaves Tauri/Wry's application-owned persistent profile in place and never deletes or resets it.
- The `test-runtime` feature continues to use only explicit `DOCUMENT_STUDIO_TEST_*` inputs and direct `WebviewWindowBuilder` data-directory/CDP arguments. Raw IPC, single-instance behavior and exact-owned cleanup remain in their accepted boundaries.

## Evidence contract

- Pure Rust classification/planning tests cover every required name, mixed casing, an unknown future name, unrelated variables and the exact replacement value.
- The real test-runtime WebView2 and single-instance smokes launch with hostile inherited runtime, profile, argument, debugger and future controls; exact builder-owned UDF/CDP behavior must still pass.
- The release smoke launches twice, rejects hostile inherited controls in owned WebView2 command lines, requires the same application-owned profile and proves the profile survives both launches. Cleanup targets only the test processes and temporary evidence; it does not remove the production profile.
- The G03 boundary verifier and repository validator enforce startup order, policy mechanics, test/runtime separation and absence of global process cleanup.

No dependency, operation contract, PDF behavior, SQLite migration, qpdf rule, Tauri capability, CSP or CI command changed.

## Local verification

- Locked validation dependency installation, repository validation, internal link check and `git diff --check` passed.
- TypeScript typecheck, 116 frontend/contracts tests and the production frontend build passed.
- Rust formatting, warning-denied workspace clippy and all default workspace tests passed. The two existing manual performance cases remained intentionally ignored.
- The policy's two focused pure Rust tests passed in default and `test-runtime` builds.
- The native single-instance smoke passed primary/secondary behavior plus all five bounded failure-path self-tests while hostile inherited WebView2 controls were present.
- The real WebView2 smoke passed exact isolated UDF/direct loopback CDP creation, raw 262,144-byte PDF IPC, stale-session rejection and G04B2 authenticated raw-pixel IPC, with exact-owned cleanup.
- Tauri release `--no-bundle` passed. The production smoke launched twice with malicious runtime, UDF, browser-argument, debugger, channel, background, future-variable and max-instance inheritance. Both launches used the same application-owned persistent profile, exposed no inherited CDP/debug switch and left that profile present.

Exact-head GitHub CI and independent focused review remain merge gates; this log does not claim them early.
