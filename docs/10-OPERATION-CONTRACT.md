# Unified Operation Contract

Every tool must implement the same lifecycle.

## Manifest fields

- `id` and semantic version.
- User-facing name, category and description.
- Accepted input types, multiplicity, page-selection rules and maximum tested sizes.
- Typed settings schema and defaults.
- Dependency and platform capabilities.
- Risk level and whether preview/confirmation is required.
- Output type and verification rules.

## Lifecycle

1. `inspect` - collect safe metadata without mutating input.
2. `preflight` - validate files, passwords, dependencies, storage and settings.
3. `estimate` - work units, likely duration class and output-size range when possible.
4. `plan` - resolve adapter, arguments and workspace paths.
5. `execute` - cancellable processing with structured progress events.
6. `verify` - reopen output and assert operation-specific invariants.
7. `publish` - atomically move verified output to the destination.
8. `audit` - store timings, versions, fingerprints, warnings and result paths.
9. `cleanup` - remove temporary data on every terminal path.

## Standard progress event

```json
{
  "jobId": "uuid",
  "operationId": "pdf.merge",
  "state": "running",
  "stage": "assembling-pages",
  "completedUnits": 32,
  "totalUnits": 80,
  "message": "Adding page 32 of 80",
  "cancellable": true
}
```

## Verification examples

- Merge: output opens, expected total pages, expected source order and no unintended encryption.
- OCR: output opens, page count preserved, text layer exists on representative/all pages as configured.
- Redaction: target regions are removed from text extraction and pixels are sanitized according to mode.
- Protect: wrong password fails, correct password opens, requested permission flags match.

## G01 `diagnostic.copy` protocol versions

- `1.0.1` is the durable-partial protocol. It validates, selects a bounded collision candidate, generates an exact random destination-local partial path, and commits that reservation and final candidate to SQLite. A reservation is not deletion authority. The runtime next creates a separate delete-on-close guard with `create_new`, records the guard's Windows file identity as the activation proof for that exact partial UUID, and creates the partial as a no-replace hard link to that identity. Document bytes are written only after the partial is reopened and its identity matches.
- Each partial is flushed, closed, reopened, size/hash verified, and moved with same-directory no-replace/write-through semantics. The final file is reopened and verified before publication evidence clears the matching partial ownership.
- Collision retries are bounded by `MAX_COLLISION_ATTEMPTS = 1000`. A retry starts only after an activated exact-identity partial was deleted through its opened handle, or the owned identity was proven absent, and the matching database value was cleared. A pre-existing file at a reserved path or guard path is preserved.
- `1.0.0` is the pre-fix development protocol. A null `partial_path` or terminal state does not prove that an earlier destination partial was removed. Unknown legacy files are never inferred from names or scanned by prefix.

## G02 `pdf.merge` 1.0.0

- Inputs: exactly 2–128 ordered `application/pdf` files. The request array, database ordinals, physical `inputs/source-NNNN.pdf` snapshots, and qpdf `--file=` arguments match one-to-one.
- Settings: an empty object with no additional properties. Output: exactly one `application/pdf`.
- Dependency: bundled qpdf exactly 12.3.2.
- Page-only policy: document metadata, bookmarks, attachments, interactive forms, and signatures are unsupported/not preserved as supported features. Existing digital signatures will not remain valid.
- Strict preflight: PDF extension and magic, regular local file, stable identity/size/modified time while copied, unencrypted, strict no-recovery structural check, and at least one page.
- Verification: owned regular staging file, plausible PDF header/size, SHA-256, strict reopen, unencrypted result, page count equal to the ordinal-aware input sum, and final publication size/hash equality.
- Lifecycle: the shared `inspect → preflight → estimate → plan → execute → verify → publish → audit → cleanup` stages. qpdf execution reports an indeterminate stage instead of inventing a percentage.
