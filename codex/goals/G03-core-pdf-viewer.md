# G03 — Core PDF and Viewer

Implement progressive PDF.js preview and virtualized thumbnails, then add split, extract, remove, reorder, rotate, linearize and PDF-to-images through the existing operation and qpdf adapter architecture.

Verification includes first-page/visible-thumbnail budgets, bounded memory, off-screen render cancellation, page-order golden tests, encrypted/malformed failures, cancellation, recovery and no duplicated job infrastructure.
