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

- Mozilla `pdfjs-dist` 6.2.108 is Apache-2.0. Document Studio ships exactly the 191 worker/CMap/standard-font/ICC/WASM files in `apps/desktop/scripts/pdfjs-assets-6.2.108.json`, verified by path, byte size and SHA-256 before atomic staging. QuickJS, no-WASM fallbacks, source maps and the generic viewer UI are excluded; no CDN is used.
- TanStack React Virtual 3.14.9 and resolved Virtual Core 3.17.7 are MIT.
- Playwright Test, Playwright and Playwright Core 1.62.1 are Apache-2.0 and are development/CI only. Project-local Chrome for Testing 151.0.7922.34, Headless Shell and helper binaries are ignored test artifacts and are not part of a production package.
- `pdfjs-dist` resolves optional `@napi-rs/canvas` 1.0.6 packages under MIT for Node environments. The desktop webview viewer does not import them; the lock still records their exact platform packages and integrity.
- The lock also contains optional platform-specific package metadata, including `fsevents`, that may declare install scripts. Document Studio does not claim such metadata is absent: approved installs and CI use `--ignore-scripts`, so package lifecycle scripts are not executed.

A distributable build must retain all applicable PDF.js asset licence/notices and regenerate the complete shipped-runtime notice inventory from the lockfile and staged asset tree. Any package or browser revision change requires a fresh dependency review.

## G04 notices

G04A adds no new package and reuses the accepted Apache-2.0 qpdf 12.3.2 bundle.

G04B's images-to-PDF writer adds these exact crates from crates.io:

- `image` 0.25.10: MIT OR Apache-2.0; only JPEG, PNG and WebP features are enabled.
- `pdf-writer` 0.15.0: MIT OR Apache-2.0; default features are disabled.
- `flate2` 1.1.9: MIT OR Apache-2.0; only the pure-Rust backend is enabled.

Their exact registry checksums and boundaries are recorded in ADR-013 and the dependency register. A distributable build must regenerate complete transitive notices from the accepted `Cargo.lock`. G04B2 reuses accepted `pdfjs-dist` 6.2.108 for rendering and `image` 0.25.10 for encoding; it does not bundle PDFium, MuPDF, Poppler, PDFBox, libvips or another raster renderer.

### G04E1 Windows COM projections

- `webview2-com` 0.38.2 is MIT, from the `wravery/webview2-rs`
  project. Document Studio declares it directly on Windows for generated
  WebView2 resource-interception and `PrintToPdf` COM interfaces. WebView2
  itself remains the Windows platform-provided Evergreen runtime; no runtime
  binary is bundled or downloaded.
- Microsoft `windows` 0.61.3 is MIT OR Apache-2.0. Document Studio enables
  only `Win32_System_Com` and `Win32_UI_Shell` for typed `IStream` and
  `SHCreateMemStream` responses.

The exact registry checksums are recorded in ADR-018 and the dependency
register. These direct edges do not add a second resolved package version.
Release packaging must regenerate the complete transitive notice inventory
from the accepted lockfile; any version or feature change requires a new
dependency review.

### G04E1 packaged Noto Regular fonts

Document Studio includes these three unmodified static hinted fonts under the SIL Open Font License 1.1:

- `NotoSans-Regular.ttf`, version 2.015, copyright 2022 The Noto Project Authors.
- `NotoSansDevanagari-Regular.ttf`, version 2.006, copyright 2022 The Noto Project Authors.
- `NotoSansTelugu-Regular.ttf`, version 2.005, copyright 2022 The Noto Project Authors.

The complete OFL-1.1 text and font-specific copyright statement accompany each file under `apps/desktop/src-tauri/resources/fonts/g04e1/licenses/`. Exact source repository/tag/commit/archive provenance, archive and file SHA-256 values, names, cmap evidence and OpenType tables are recorded in `font-manifest.json` and the dependency register. The fonts are not installed globally, altered, pre-subset or downloaded at runtime; Chromium PDF subset embedding is the only permitted subsetting.

### G04C2 public-domain photographic test corpus

G04C2 commits six 1280px JPEG derivatives by George Chernilevsky solely as offline quality and compression fixtures. Each source page marks the work PD-self/Public domain. Exact revision identity, resolved binary URL, dimensions, byte size and SHA-256 are preserved in the corpus manifest.

- [Sunflower head 2015 G2](https://commons.wikimedia.org/w/index.php?title=File:Sunflower_head_2015_G2.jpg&oldid=1251267196)
- [Folk Architecture 2015 G07](https://commons.wikimedia.org/w/index.php?title=File:Folk_Architecture_2015_G07.jpg&oldid=1250325699)
- [Lviv Church of the Dormition 2015 G1](https://commons.wikimedia.org/w/index.php?title=File:Lviv_Church_of_the_Dormition_2015_G1.jpg&oldid=1098857210)
- [Uzh River near Chernobyl 2019 G2](https://commons.wikimedia.org/w/index.php?title=File:Uzh_River_near_Chernobyl_2019_G2.jpg&oldid=1110946301)
- [Thorichthys meeki 2019 G1](https://commons.wikimedia.org/w/index.php?title=File:Thorichthys_meeki_2019_G1.jpg&oldid=1238638582)
- [Fruit on a plate 2019 G1](https://commons.wikimedia.org/w/index.php?title=File:Fruit_on_a_plate_2019_G1.jpg&oldid=1262411860)
