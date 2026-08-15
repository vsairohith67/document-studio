# Dependency and License Register

The dependency register distinguishes **adopt**, **evaluate**, **optional service** and **inspiration only**. Versions are verified at implementation time and pinned with checksums.

| Component | Use | Status | License/notes |
|---|---|---|---|
| Tauri 2 | Desktop shell and secure IPC | Adopt | MIT/Apache-2.0 |
| React 19.2+ | UI | Adopt | MIT; use patched 19.2.x or later |
| Vite 8.1+ | Front-end build | Adopt | MIT |
| PDF.js 5.7+ | Viewer, text layer, annotations | Adopt | Apache-2.0 |
| qpdf 12.3+ | Structural PDF transforms/encryption | Adopt | Apache-2.0 |
| libvips 8.18+ | Image conversion/compression | Adopt | LGPL-2.1-or-later; distribution review |
| OCRmyPDF 17.4+ | OCR orchestration | Adopt optional dependency | MPL-2.0 |
| Tesseract 5.5+ | OCR engine/language packs | Adopt | Apache-2.0 |
| LibreOffice 26.2+ | Office conversion | Adopt external dependency | MPL-2.0 and bundled notices |
| Gotenberg 8.32+ | Server-side Office/HTML conversion | Optional web service | MIT; container deployment |
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
