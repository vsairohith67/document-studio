# Unified Feature Catalogue

The canonical machine-readable catalogue is `feature-catalog.csv`. It currently contains **132 planned capabilities**. Phase numbers are implementation order, not access tiers.

## Phase summary

- **Phase 0:** application foundation, privacy, accessibility, diagnostics, job lifecycle and history.
- **Phase 1:** structural PDF operations and basic image conversion.
- **Phase 2:** compression, Office conversion, overlays, batch processing and protection.
- **Phase 3:** editing workspace, forms, signatures and OCR.
- **Phase 4:** true redaction, sanitization, structured extraction and comparison foundations.
- **Phase 5+:** archival, digital signatures, advanced conversion, workflows, AI, web/mobile and plugin platform.

## Product rule

Every implemented feature is available in one complete personal edition. “Must/Should/Could” expresses implementation priority only.

## G02 implemented slice

PDF Merge is the first Phase 1 production operation. It accepts 2–128 local PDFs, keeps the exact user-visible order including deliberate duplicates, and publishes one verified page-only PDF without overwrite. Page ranges, image inputs, bookmark or outline preservation, a viewer, and thumbnails are not part of G02.
