# Dependency and License Register

The dependency register distinguishes **adopt**, **evaluate**, **optional service** and **inspiration only**. Versions are verified at implementation time and pinned with checksums.

| Component | Use | Status | License/notes |
|---|---|---|---|
| Tauri 2.11.x | Desktop shell and secure IPC | Adopt | MIT/Apache-2.0; CLI 2.11.4 and JS API 2.11.1 were current at the 22 July 2026 recheck |
| React 19.2.7+ | UI | Adopt | MIT; pin a patched 19.2.x or later |
| Vite 8.1.5+ | Front-end build | Adopt | MIT; Vite 8 uses Rolldown; pair with `@vitejs/plugin-react` 6.x |
| PDF.js 5.7.284+ | Viewer, text layer, annotations | Adopt | Apache-2.0 |
| qpdf 12.3.2+ | Structural PDF transforms/encryption | Adopt | Apache-2.0; verify release signatures/checksums |
| libvips 8.18.2+ | Image conversion/compression | Adopt | LGPL-2.1-or-later; dynamic-linking/distribution review |
| OCRmyPDF 17.8.x | OCR orchestration | Evaluate as managed optional worker | MPL-2.0; native Windows needs Python, Tesseract and Ghostscript; packaging and sandboxing gate required |
| Tesseract 5.5.2+ | OCR engine/language packs | Adopt | Apache-2.0; Windows installer is third-party, so binary provenance must be governed |
| LibreOffice 26.2.4+ | Office conversion | Adopt external dependency | MPL-2.0 and bundled notices; detect installed version and isolate profile directories |
| Gotenberg 8.32+ | Server-side Office/HTML conversion | Optional web service | MIT; container deployment; keep patched because conversion surfaces process untrusted files |
| pdf-lib | Browser overlays/simple creation | Evaluate/adopt for narrow cases | MIT; not full arbitrary editing/decryption |
| pdfcpu 0.12+ | Go-side PDF operations/signature evaluation | Evaluate | Apache-2.0; project describes itself as evolving |
| veraPDF | PDF/A validation | Evaluate | dual-license/distribution review required |
| Docling | Structured document parsing | Evaluate | MIT; model licenses separate |
| Granite Docling 258M | Local document structure extraction | Evaluate | Apache-2.0; English model |
| multilingual-e5-small | Multilingual embeddings | Evaluate | MIT; 512-token chunk limit |
| UI UX Pro Max | Design guidance for Codex | Adopt as development skill | MIT; install through `uipro-cli` |
| Tokens Studio | Figma design-token sync | Recommend | MIT |
| Iconify | Icon exploration/import | Recommend with icon-set license review | Plugin/repository terms plus individual set licenses |
| Design Lint | Figma consistency checks | Recommend | MIT |
| Stirling-PDF | Feature inspiration/benchmarking | Inspiration only | Do not vendor or copy; separate open-core/security considerations |

## Adoption gate

A dependency is not production-approved until licensing, update channel, binary provenance, sandboxing, performance, failure behavior and output verification are documented in an ADR.

## Version-policy note

The versions above are recheck baselines, not permanent pins. Before each release, update the dependency lock, verify upstream signatures/checksums and advisories, run the compatibility matrix, and record the adopted revision in an ADR and software bill of materials.
