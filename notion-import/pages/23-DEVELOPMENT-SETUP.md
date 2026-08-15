# Development Setup and First Build

This is the target-machine runbook for the Windows-first Phase 0 build. It intentionally separates **development prerequisites**, **core engine packages**, and **later optional capabilities** so Codex does not install a large, fragile toolchain before it is needed.

## Target baseline

- Windows 11 x64, current security updates.
- 16 GB RAM minimum; 32 GB recommended for OCR, very large documents and local models.
- SSD with at least 20 GB free for toolchains, build caches, fixtures and temporary jobs.
- A private GitHub repository and a non-administrator day-to-day development account.

## 1. Install Tauri development prerequisites

Tauri requires Microsoft C++ Build Tools and Microsoft Edge WebView2 on Windows. In Visual Studio Build Tools, enable **Desktop development with C++**. WebView2 is normally already present on current Windows 10/11 systems. MSI packaging may also require the Windows VBSCRIPT optional feature.

Install the remaining tools from an elevated PowerShell window:

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
winget install --id OpenJS.NodeJS.LTS -e
winget install --id Python.Python.3.12 -e
rustup default stable-msvc
```

Restart the terminal, then verify:

```powershell
git --version
node --version
npm --version
rustc --version
cargo --version
python --version
```

## 2. Clone and validate the planning repository

```powershell
git clone <PRIVATE_REPOSITORY_URL> document-studio
cd document-studio
python scripts/validate_repo.py
python scripts/check_links.py
npm install
npm run typecheck
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm --workspace apps/desktop run tauri dev
```

The first successful checkpoint is the starter window plus the `system_status` Tauri command. Do not add PDF engines until this baseline builds and tests cleanly.

## 3. Core engine installation policy

### qpdf

Use the signed official Windows release. Verify its published checksum/signature, store provenance in `third_party/manifest.json`, and invoke it through validated argument arrays. It is the Phase 1 engine for structural transforms, encryption, linearization and integrity checks.

### libvips

Use a reviewed Windows binary or a reproducible build. Prefer dynamic linking and ship the required LGPL notices/source-offer information. Use it for image conversion, thumbnails and image-heavy compression paths.

### PDF.js

Install through npm. Keep rendering in the UI process, but move expensive page inspection and document operations to workers/Rust commands. Use virtualized thumbnails and progressive page rendering.

## 4. Optional capability dependencies

### LibreOffice

Treat LibreOffice as a separately installed dependency during development. Detect its version/path, create an isolated user profile per worker, impose time/memory limits and verify every converted output. Decide later whether the installer downloads it, bundles a permitted subset, or requires the user to install it.

### OCRmyPDF and Tesseract

Do not require OCR in Phase 0. For the OCR milestone:

1. Benchmark direct Tesseract + controlled PDF assembly against OCRmyPDF.
2. For OCRmyPDF on native Windows, provision 64-bit Python, Tesseract and Ghostscript in a managed worker environment.
3. Treat Windows third-party Tesseract binaries as a provenance risk; record source, checksum and update channel.
4. Install only the requested language packs (`eng`, `hin`, `tel`) and verify checksums.
5. Keep WSL/Docker as development or optional-server paths, not silent desktop requirements.

### Local AI models

No model is installed during the foundation. The model manager must require user action, pin an immutable revision, verify checksums, show disk/memory requirements and allow removal. Run the benchmarks in `models/models.yaml` before enabling any model-backed feature.

## 5. Phase 0 command sequence for Codex

```text
1. Open the repository root in Codex.
2. Read AGENTS.md, CODEX_START_HERE.md and FINAL_RECHECK.md.
3. Run codex/prompts/01-foundation.md only.
4. Make one vertical slice at a time.
5. Run format, typecheck, Rust tests, repository validators and a Tauri smoke test.
6. Record command output and unresolved decisions under docs/implementation-log/.
7. Stop when the Phase 0 acceptance gate passes.
```

## 6. First-build evidence to retain

- Exact Node/npm/Rust/Cargo versions.
- Generated lockfiles and Cargo.lock.
- `npm run typecheck` output.
- `cargo test` output.
- Tauri dev-launch screenshot.
- Dependency diagnostic screen output.
- Phase 0 acceptance checklist and any deviations.

Never mark the toolchain ready based on installation alone; preserve reproducible evidence that the starter builds and runs on the target machine.
