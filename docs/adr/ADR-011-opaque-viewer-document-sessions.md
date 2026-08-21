# ADR-011: Opaque retained-handle viewer sessions

Status: Accepted; implemented in G03

## Context

PDF.js needs progressive random access to large local PDFs. Raw paths, `file://`, whole-file JSON/base64 IPC, a custom protocol, and broad filesystem capabilities would weaken the accepted boundary.

## Decision

Rust opens one validated regular non-reparse PDF through a retained read-only Windows handle that denies write and delete sharing. The handle and canonical identity record remain behind an unpredictable session ID and generation. React receives only sanitized display metadata, size, time, MIME type, opaque identity and the opaque session values; it never receives the path.

`viewer_read_range` returns `tauri::ipc::Response` bytes. The normal PDF.js chunk is 256 KiB, the command rejects chunks above 1 MiB, and PDF.js permits at most four reads in flight. Reads use positioned `seek_read` against the retained handle, so overlapping and repeated ranges are allowed and there is no shared seek cursor. Tauri classifies `Response::new(Vec<u8>)` as `InvokeResponseBody::Raw`; responses above its 1 KiB direct-execute threshold use `application/octet-stream`, not Document Studio JSON serialization or base64.

Opening uses the backend native PDF dialog. Viewer drag/drop uses Tauri's Rust-side `WindowEvent::DragDrop`/`DragDropEvent`; Rust validates and opens the path, then emits only sanitized viewer metadata. The accepted G01/G02 commands, IPC v1 and `dialog:allow-open` remain unchanged alongside the viewer APIs.

Sessions are limited to two, expire when idle, detect source identity/size/time changes, reject stale generations, support explicit close, and close on window/application shutdown. Closing invalidates future reads; in-flight responses are generation-checked before return. The same retained handle creates an ASCII-named durable-job snapshot. Opaque destination grants hold a validated directory record and can be revoked.

The resulting durable G03 job necessarily keeps trusted source and publication paths for processing and recovery. IPC redacts those fields for G03 create/get/history/recovery responses while preserving safe filenames, status and verification metadata. Existing G01/G02 responses are not changed.

## Alternatives rejected

- Whole-file `Uint8Array` IPC creates avoidable peak memory and prevents progressive range loading.
- A custom local protocol creates a larger URL/CSP/trust boundary and was not approved.
- `file://`, raw paths, a local HTTP server, shell, general filesystem access and custom COM drop are prohibited.

## Consequences

The viewer session is ephemeral and never becomes job history. Output work creates a separate durable job only at Apply/Export. A future need for a custom protocol or native COM drop requires a new approval gate and ADR.
