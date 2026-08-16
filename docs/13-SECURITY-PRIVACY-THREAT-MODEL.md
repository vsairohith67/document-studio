# Security, Privacy and Threat Model

## Trust boundaries

- Untrusted documents and archives.
- UI/webview process.
- Tauri/Rust trusted core.
- Third-party document engines.
- Optional external AI/cloud providers.
- User filesystem and OS credential store.

## Required controls

- Allow-list executables and operation arguments.
- Pass arguments as arrays; never concatenate shell strings.
- Canonicalize paths and enforce destination/workspace roots.
- Reject path traversal, symlink escape and dangerous archive entries.
- Isolate each job; use least privilege and resource limits where supported.
- Validate magic bytes and parser output, not only extensions.
- Patch bundled engines and record exact versions/hashes.
- Treat PDFs, images, fonts, Office files and archives as attacker-controlled.
- Disable network by default for document processing.
- Require per-job consent for external processing and show provider/data scope.
- Never log passwords, keys, document text or raw model prompts by default.
- Verify signed updates and bundled sidecar checksums.

## G01 Windows controls

- Accept one existing regular local-disk input and one user-selected local destination for `diagnostic.copy`.
- Reject UNC/device namespace paths, alternate data streams, reserved DOS names, traversal, trailing-dot/space names and every reparse component.
- Compare Windows file identities so the destination cannot alias the input through a hard link.
- Keep each job under a random UUID workspace with an application ownership marker. Cleanup verifies the canonical application root, UUID and marker before exact removal.
- Open the source with sharing that denies mutation/deletion while copying. Stream fixed-size chunks and never load the whole document.
- Create destination-local random partial files with create-new semantics. Flush, independently hash, then move in the same directory with no-replace/write-through flags.
- Resolve collisions as `name-copy.ext`, then numbered suffixes. A commit-time race preserves the competing file and retries safely.
- Allow cancellation between bounded chunks and stages. The final no-replace move is a small, explicit non-cancellable commit boundary.
- Complete only after verification, durable publication evidence, audit persistence and confirmed temporary-data cleanup. Ambiguity remains `interrupted`.
- Expose only Tauri core defaults and the native open-dialog permission. G01 has no shell, general webview filesystem or HTTP capability.

Tests place document and password sentinels in inputs and confirm they do not appear in SQLite, events, errors or dependency diagnostics.

## Redaction safety

Visual cover-up is not redaction. The operation must remove underlying text/objects or rasterize/sanitize the affected area, then test extraction and visual output. The UI requires a final irreversible confirmation.

## Privacy modes

- Normal local mode.
- Strict offline mode (all network disabled, including update/model checks).
- External-provider mode enabled per feature/provider with explicit scope.
