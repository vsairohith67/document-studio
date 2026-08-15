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
