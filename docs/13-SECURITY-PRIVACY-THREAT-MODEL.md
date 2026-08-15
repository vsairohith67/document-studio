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

## Redaction safety

Visual cover-up is not redaction. The operation must remove underlying text/objects or rasterize/sanitize the affected area, then test extraction and visual output. The UI requires a final irreversible confirmation.

## Privacy modes

- Normal local mode.
- Strict offline mode (all network disabled, including update/model checks).
- External-provider mode enabled per feature/provider with explicit scope.
