# Third-Party Notices and Adoption Gate

This repository does not bundle production document-engine binaries or model weights. G01 does bundle the public-domain SQLite C source through the MIT-licensed `rusqlite` crate. Its direct application and test dependencies are adopted from the official npm, crates.io, and PyPI registries under the licences recorded in the dependency register. Registry integrity data is preserved in committed lockfiles.

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
