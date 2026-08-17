# Development Setup and First Build

This is the target-machine runbook for the Windows-first Phase 0 build. It intentionally separates **development prerequisites**, **core engine packages**, and **later optional capabilities** so Codex does not install a large, fragile toolchain before it is needed.

## Target baseline

- Windows 11 x64, current security updates.
- 16 GB RAM minimum; 32 GB recommended for OCR, very large documents and local models.
- SSD with at least 20 GB free for toolchains, build caches, fixtures and temporary jobs.
- A private GitHub repository and a non-administrator day-to-day development account.

## 1. Install Tauri development prerequisites

Tauri requires Microsoft C++ Build Tools and Microsoft Edge WebView2 on Windows. In Visual Studio Build Tools, enable **Desktop development with C++**. WebView2 is normally already present on current Windows 10/11 systems. MSI packaging may also require the Windows VBSCRIPT optional feature.

Install the remaining tools only when they are missing and after reviewing the exact command. G01 was accepted with Node 24.19.0, npm 11.17.0, Rust/Cargo 1.97.1 on `stable-x86_64-pc-windows-msvc`, and Python 3.12.10. The repository records these versions in `.node-version`, `.python-version`, `rust-toolchain.toml` and `package.json`.

Example bootstrap commands for a new machine are:

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
winget install --id OpenJS.NodeJS -e
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

## 2. Clone and validate the G01 repository

```powershell
git clone <PRIVATE_REPOSITORY_URL> document-studio
cd document-studio
.\.venv\Scripts\python.exe -m pip install --require-hashes --only-binary=:all: -r scripts\requirements-validation.lock.txt
npm ci --ignore-scripts
.\.venv\Scripts\python.exe -B scripts\validate_repo.py
.\.venv\Scripts\python.exe -B scripts\check_links.py
npm run typecheck
npm test
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
npm --workspace @document-studio/desktop run tauri -- build --no-bundle
npm --workspace @document-studio/desktop run tauri -- dev
```

The launched window shows the production PDF Merge workspace. Add 2–128 local PDFs, reorder or remove them, choose a destination and safe `.pdf` name, then merge. The runtime uses only the bundled qpdf and never downloads an engine.

`pip-tools==7.6.0` is needed only when regenerating the Python lock. On the accepted machine it required project-local `pip==26.1`; runtime validation needs only the committed hash-locked PyYAML package.

## 3. Core engine installation policy

### qpdf

G02 uses only the reviewed bundle under `apps/desktop/src-tauri/resources/qpdf/12.3.2`. Verify it without downloading anything:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-qpdf-bundle.ps1 -BundleRoot .\apps\desktop\src-tauri\resources\qpdf\12.3.2
& .\apps\desktop\src-tauri\resources\qpdf\12.3.2\bin\qpdf.exe --version
cargo test -p document-studio --locked --features test-runtime --test process_sandbox -- --test-threads=1
```

The acquisition script is for an explicitly reviewed dependency update only. It refuses an existing destination, downloads only the three pinned official assets, requires Cosign 3.0.6, verifies provenance and archive hash, and emits the exact reviewed runtime tree. Never put qpdf on PATH or invoke it through a shell.

### libvips

Use a reviewed Windows binary or a reproducible build. Prefer dynamic linking and ship the required LGPL notices/source-offer information. Use it for image conversion, thumbnails and image-heavy compression paths.

### PDF.js

PDF.js is not installed in G01. Adopt and review it in G03 when the viewer is implemented. Keep rendering in the UI process, but move expensive inspection and document operations to workers or Rust commands.

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
- Committed `package-lock.json`, `Cargo.lock` and hash-locked Python validation requirements.
- `npm run typecheck` output.
- `cargo test` output.
- Tauri dev-launch screenshot.
- Dependency diagnostic screen output.
- Phase 0 acceptance checklist and any deviations.

Never mark the toolchain ready based on installation alone; preserve reproducible evidence that the starter builds and runs on the target machine.
