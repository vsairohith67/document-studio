# Local Validation Evidence

The following checks were executed on 22 July 2026 against the final package:

- `repository-validation.txt`: required repository structure and exactly 132 feature entries.
- `markdown-links.txt`: internal Markdown links.
- `prototype-node-check.txt`: JavaScript syntax (empty output means success).
- `prototype-http-smoke.txt`: static prototype served over HTTP with Home/workbench markup.
- `typescript-syntax.txt`: dependency-independent TypeScript/TSX syntax transpilation; this is not full typechecking.
- `python-compile.txt`: Python validation/report scripts compile.
- `go-tests.txt`: future cloud control-plane skeleton unit test.
- `data-parse.txt`: JSON, YAML and TOML parsing, including GitHub workflow/templates.
- `docx-unzip.txt`: DOCX ZIP integrity.
- `docx-a11y.json`: accessibility audit with zero high/medium/low findings.
- `docx-metadata.txt`: all eight figures have alt descriptions.
- `pdf-verification.txt`: canonical 44-page PDF openability and first/last-page text checks.
- `pdfinfo.txt` and `pdftotext-stats.txt`: PDF metadata and text extraction.
- `canonical-name-scan.txt`: no retired product name in current sources; historical archive excluded intentionally.
- `attachment-parity.txt`: canonical report hashes match the Notion-import copies.
- `final-validation-status.txt`: final local validation gate.
- `npm-install-status.txt`: npm registry resolution exceeded a 240-second retry; no partial dependencies are included.

The master DOCX was rendered to 44 page PNGs and every page was visually reviewed before the QA render directories were removed from the deliverable.

Rust/Cargo and qpdf are not installed in this execution environment. Full dependency-resolved frontend typechecking/build, Tauri/Rust compilation, PDF-engine adapter tests and Windows packaging remain target-machine/CI gates rather than claimed local results.
