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

## G03 implemented slice

G03 adds one local-PDF viewer with progressive virtual pages/thumbnails, navigation, zoom/fit, temporary view rotation, selectable text and bounded incremental search. It adds one organizer for unique page selection, exact permutation reorder, removal with at least one page retained, 90/180/270-degree output rotation, extraction in selected order, and split by every page, fixed count or an explicit full-document non-overlapping partition. Split is limited to 128 outputs.

The viewer does not promise OCR or preservation/editing of bookmarks, attachments, forms, annotations or signatures. G03 does not include linearization, image conversion, compression, repair, crop, watermark, page numbering, freeform editing, printing, cloud or AI.
