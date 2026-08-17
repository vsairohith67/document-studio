# Screen Specifications

## Home

- Greeting and compact privacy status.
- Universal “Search tools and commands” field.
- 640-760 px wide drop zone with local/cloud explanation.
- Eight configurable quick-tool cards.
- Recent jobs table with status, operation, time, output and action menu.
- Saved workflow cards with step chips and run button.

## Tool setup screen

- Files list with validation state.
- Operation explanation in one sentence.
- Smart defaults selected.
- Advanced section collapsed.
- Output path and naming template always visible.
- “Run” button states exact action, e.g., “Merge 8 pages.”

## Workbench

- Virtualized thumbnails for large files.
- First page appears while remaining previews render progressively.
- Canvas supports fit page/width, zoom, search and keyboard page navigation.
- Inspector uses sections: Selection, Operation, Output, Verification.
- Dangerous choices use warning copy, not only color.

## Result screen

- Verified status and checks performed.
- Output path, size, page count and processing time.
- Open, reveal, compare, run another and add-to-workflow actions.
- Warnings remain visible; success is not shown when verification failed.

## Settings

- General, Appearance, Privacy, Storage, Dependencies, OCR languages, AI providers, Updates, Accessibility, Diagnostics.
- Global network disable switch.
- Clear history/temporary storage with scope preview.

## G02 PDF Merge workspace

- “Add PDFs” supports native multi-select and safe native file drop.
- The ordered list shows number, name, byte size, modified time, validation state, and an intentional-duplicate badge.
- Every row has Move up, Move down, and Remove. `Alt+Up`, `Alt+Down`, and `Delete` provide the same keyboard behavior. Reorder and removal are announced through an `aria-live` region.
- The destination chooser selects an existing local folder. The filename must be a safe Windows `.pdf` name.
- Merge stays disabled until 2–128 valid PDFs, a destination, a valid name, and verified qpdf 12.3.2 are available.
- Progress uses real lifecycle stages and byte/item counts. The qpdf merge phase is indeterminate because qpdf does not provide a trustworthy percentage.
- Cancel remains available until safe publication commit begins. Success shows the actual collision-resolved path; failures show sanitized actions.
- A standing notice says document metadata, bookmarks, attachments, interactive forms, and signatures are unsupported/not preserved as supported features, and existing digital signatures will not remain valid.
