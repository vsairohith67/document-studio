# Third-Party Notices and Adoption Gate

G02 bundles the reviewed qpdf 12.3.2 production PDF engine and its required Windows runtime files. G03 packages local PDF.js 6.2.108 worker/font/CMap/ICC/WASM assets. No model weights or unrelated production document engines are bundled. G01 also bundles the public-domain SQLite C source through the MIT-licensed `rusqlite` crate. Direct application and test dependencies are adopted from the official npm, crates.io, and PyPI registries under the licences recorded in the dependency register. Registry integrity data is preserved in committed lockfiles.

The authoritative register is `docs/14-DEPENDENCY-AND-LICENSE-REGISTER.md`. Model-specific governance is in `docs/15-HUGGING-FACE-MODEL-PLAN.md` and `models/models.yaml`.

A dependency may move from evaluation to adoption only after an ADR records:

- exact version or immutable revision;
- source and binary provenance;
- license and redistribution obligations;
- vulnerability/update policy;
- sandboxing and failure behavior;
- benchmark and output-verification evidence.

## G01 direct dependency notices

- Tauri, its CLI/build crate, API, dialog plugin, and official `tauri-plugin-single-instance` 2.4.3: MIT OR Apache-2.0.
- React, React DOM, Vite, Vitest, jsdom, Testing Library, AJV, AJV Formats, and TypeScript: MIT or Apache-2.0 as recorded in the register.
- axe-core: MPL-2.0; used only by tests and not shipped in the desktop runtime bundle.
- serde, serde_json, uuid, thiserror, chrono, sha2, windows-sys, tempfile, and rusqlite: MIT or MIT OR Apache-2.0 as recorded in the register.
- bundled SQLite: public domain.
- junction: MIT; Windows tests only.
- PyYAML: MIT; repository validation only.
- pip-tools: BSD-3-Clause; lock-generation tooling only.

No G01 dependency authorizes copying third-party product code. Full transitive notices and shipped-runtime review must be regenerated from the accepted lockfiles before a distributable installer is released.

## G02 qpdf dependency notice

Document Studio bundles qpdf 12.3.2 for the local PDF Merge operation under Apache-2.0. The runtime comes only from the signed official `qpdf-12.3.2-msvc64.zip` archive. The reviewed bundle retains the full Apache-2.0 license, qpdf's upstream license pages, signed checksum provenance, and an exact size/SHA-256 manifest for every shipped runtime file.

The bundled Microsoft Visual C++ 14.44.35211 runtime DLLs are the unmodified redistributable files included with the signed upstream qpdf Windows archive. The exact filenames and hashes are recorded in `resources/qpdf/12.3.2/qpdf-manifest.json`. Release packaging must continue to carry the qpdf license materials and comply with the applicable Microsoft Visual Studio redistributable terms; removing or substituting these files requires a new dependency review.

## G03 viewer and test dependency notices

- Mozilla `pdfjs-dist` 6.2.108 is Apache-2.0. Document Studio ships its locally staged worker, CMaps, standard fonts, ICC profiles and WASM assets; it does not ship the generic viewer UI or fetch a CDN.
- TanStack React Virtual 3.14.9 and resolved Virtual Core 3.17.7 are MIT.
- Playwright Test, Playwright and Playwright Core 1.62.1 are Apache-2.0 and are development/CI only. Project-local Chrome for Testing 151.0.7922.34, Headless Shell and helper binaries are ignored test artifacts and are not part of a production package.
- `pdfjs-dist` resolves optional `@napi-rs/canvas` 1.0.6 packages under MIT for Node environments. The desktop webview viewer does not import them; the lock still records their exact platform packages and integrity.

A distributable build must retain all applicable PDF.js asset licence/notices and regenerate the complete shipped-runtime notice inventory from the lockfile and staged asset tree. Any package or browser revision change requires a fresh dependency review.
