from pathlib import Path
import csv
import json
import re
import yaml

ROOT = Path(__file__).resolve().parents[1]
required = [
    'README.md',
    'CODEX_START_HERE.md',
    'AGENTS.md',
    'FINAL_RECHECK.md',
    'MANIFEST.json',
    'CONTRIBUTING.md',
    'SECURITY.md',
    'CHANGELOG.md',
    'THIRD_PARTY_NOTICES.md',
    '.github/PULL_REQUEST_TEMPLATE.md',
    '.github/ISSUE_TEMPLATE/bug_report.yml',
    '.github/ISSUE_TEMPLATE/feature_request.yml',
    'docs/adr/README.md',
    'docs/00-AUDIT-AND-COMPLETION.md',
    'docs/22-IMPLEMENTATION-READINESS-CHECKLIST.md',
    'docs/23-DEVELOPMENT-SETUP.md',
    'docs/24-FINAL-RECHECK.md',
    'docs/25-GOAL-MODE-BUILD-PLAYBOOK.md',
    'codex/goals/README.md',
    'codex/goals/G00-readiness-audit.md',
    'codex/goals/G01-foundation.md',
    'codex/goals/G02-pdf-merge.md',
    'codex/goals/G03-core-pdf-viewer.md',
    'codex/goals/G04-optimize-convert.md',
    'diagrams/goal-mode-execution.svg',
    'notion-import/pages/25-GOAL-MODE-BUILD-PLAYBOOK.md',
    'docs/feature-catalog.csv',
    'packages/contracts/operation.schema.json',
    'packages/contracts/job.schema.json',
    'packages/contracts/ipc.schema.json',
    'packages/contracts/package.json',
    'packages/contracts/fixtures/foundation-contracts.json',
    'packages/tokens/document-studio.tokens.json',
    'packages/tokens/package.json',
    'models/models.yaml',
    'apps/prototype/index.html',
    'apps/prototype/screenshots/home.png',
    'apps/prototype/screenshots/workbench.png',
    'apps/desktop/package.json',
    'apps/desktop/src-tauri/Cargo.toml',
    'apps/desktop/src-tauri/migrations/0001_metadata.sql',
    'apps/desktop/src-tauri/migrations/0002_jobs.sql',
    'apps/desktop/src-tauri/migrations/0003_workflows.sql',
    'apps/desktop/src-tauri/migrations/0004_job_operation_plans.sql',
    'apps/desktop/src-tauri/migrations/0005_job_operation_specs_and_warnings.sql',
    'docs/adr/ADR-005-storage-ownership-and-provider-persistence.md',
    'docs/adr/ADR-006-foundation-dependencies-and-sqlite.md',
    'docs/adr/ADR-007-durable-publication-and-recovery.md',
    'docs/adr/ADR-010-pdfjs-local-rendering-security.md',
    'docs/adr/ADR-011-opaque-viewer-document-sessions.md',
    'docs/adr/ADR-012-versioned-page-plans-and-multi-output-publication.md',
    'docs/adr/ADR-013-g04b-image-pdf-conversion-dependencies.md',
    'docs/implementation-log/G01-foundation.md',
    'docs/implementation-log/G03-viewer-core-pdf.md',
    'docs/implementation-log/G04B-image-pdf-conversion.md',
    'docs/implementation-log/G04B2-pdf-to-images.md',
    'docs/implementation-log/assets/g01-tauri-dev-launch.png',
    'Cargo.toml',
    'Cargo.lock',
    'rust-toolchain.toml',
    'package-lock.json',
    'scripts/requirements-validation.in',
    'scripts/requirements-validation.lock.txt',
    'figma/README.md',
    'codex/prompts/01-foundation.md',
    'report/Document_Studio_Master_Blueprint.docx',
    'report/Document_Studio_Master_Blueprint.pdf',
    'notion-import/attachments/current/Document_Studio_Master_Blueprint.docx',
    'notion-import/attachments/current/Document_Studio_Master_Blueprint.pdf',
    'notion-import/pages/23-DEVELOPMENT-SETUP.md',
    'notion-import/pages/24-FINAL-RECHECK.md',
]
missing = [p for p in required if not (ROOT / p).exists()]
if missing:
    raise SystemExit('Missing required files: ' + ', '.join(missing))

for p in [
    'MANIFEST.json',
    'apps/desktop/package.json',
    'apps/desktop/src-tauri/tauri.conf.json',
    'apps/desktop/src-tauri/capabilities/default.json',
    'package.json',
    'package-lock.json',
    'packages/contracts/ipc.schema.json',
    'packages/contracts/fixtures/foundation-contracts.json',
    'packages/contracts/package.json',
    'packages/contracts/operation.schema.json',
    'packages/contracts/job.schema.json',
    'packages/tokens/document-studio.tokens.json',
]:
    json.loads((ROOT / p).read_text(encoding='utf-8'))

yaml.safe_load((ROOT / 'models/models.yaml').read_text(encoding='utf-8'))

manifest = json.loads((ROOT / 'MANIFEST.json').read_text(encoding='utf-8'))
if manifest.get('product') != 'Document Studio':
    raise SystemExit('MANIFEST.json does not use the canonical product name')

qpdf_resource_manifest = ROOT / 'apps/desktop/src-tauri/resources/qpdf/12.3.2/qpdf-manifest.json'
if qpdf_resource_manifest.exists():
    notices = (ROOT / 'THIRD_PARTY_NOTICES.md').read_text(encoding='utf-8')
    stale_dependency_claim = 'This repository does not bundle production document-engine binaries or model weights.'
    if stale_dependency_claim in notices:
        raise SystemExit('THIRD_PARTY_NOTICES.md denies the manifest-controlled bundled qpdf runtime')

    g02_state_documents = [
        ROOT / 'MANIFEST.json',
        ROOT / 'codex/goals/G02-pdf-merge.md',
        ROOT / 'docs/implementation-log/G02-pdf-merge.md',
    ]
    stale_g02_claims = [
        'G02 READY TO STAGE',
        'No commit, push or release has occurred.',
        'with no staging, commit, push or release.',
        'commit, push, or PR was added',
    ]
    for document in g02_state_documents:
        contents = document.read_text(encoding='utf-8')
        for stale_claim in stale_g02_claims:
            if stale_claim in contents:
                raise SystemExit(
                    f'Stale G02 repository-state claim found in {document.relative_to(ROOT)}: {stale_claim}'
                )

with (ROOT / 'docs/feature-catalog.csv').open(encoding='utf-8') as f:
    rows = list(csv.DictReader(f))
if len(rows) != 132:
    raise SystemExit(f'Expected exactly 132 feature entries; found {len(rows)}')

package = json.loads((ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8'))
if not re.fullmatch(r'[~^]?6\.[0-9]+\.[0-9]+', package.get('devDependencies', {}).get('@vitejs/plugin-react', '')):
    raise SystemExit('Vite 8 starter must use @vitejs/plugin-react 6.x')

root_package = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))
expected_workspaces = {
    'apps/desktop',
    'packages/contracts',
    'packages/tokens',
}
if set(root_package.get('workspaces', [])) != expected_workspaces:
    raise SystemExit('Root npm workspaces must contain only the three G01 packages')
for script in ['typecheck', 'test', 'build']:
    if script not in root_package.get('scripts', {}):
        raise SystemExit(f'Root package is missing the {script!r} script')

tauri_config = json.loads((ROOT / 'apps/desktop/src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
if tauri_config.get('productName') != 'Document Studio':
    raise SystemExit('Tauri product name changed from Document Studio')
if tauri_config.get('identifier') != 'studio.document.app':
    raise SystemExit('Tauri application identifier changed')

capability = json.loads((ROOT / 'apps/desktop/src-tauri/capabilities/default.json').read_text(encoding='utf-8'))
if set(capability.get('permissions', [])) != {'core:default', 'dialog:allow-open'}:
    raise SystemExit('G01 capability must expose only core defaults and dialog open')

styles = (ROOT / 'apps/desktop/src/styles.css').read_text(encoding='utf-8')
if re.search(r'#[0-9a-fA-F]{3,8}\b', styles):
    raise SystemExit('Desktop CSS duplicates a hex color instead of consuming design tokens')

for source in (ROOT / 'apps/desktop/src').rglob('*'):
    if source.suffix.lower() not in {'.ts', '.tsx', '.css'}:
        continue
    if re.search(r'\brohith?\b', source.read_text(encoding='utf-8', errors='ignore'), re.IGNORECASE):
        raise SystemExit(f'Personal UI content found in {source.relative_to(ROOT)}')

manifest_text = '\n'.join([
    (ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8'),
    (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text(encoding='utf-8'),
])
for prohibited in ['qpdf', 'libvips', 'ocrmypdf', 'tesseract', 'libreoffice']:
    if re.search(rf'(^|["\s]){re.escape(prohibited)}(["\s=:]|$)', manifest_text, re.IGNORECASE | re.MULTILINE):
        raise SystemExit(f'Out-of-scope G03 dependency found: {prohibited}')

approved_g03_dependencies = {
    'pdfjs-dist': '6.2.108',
    '@tanstack/react-virtual': '3.14.9',
}
for dependency, version in approved_g03_dependencies.items():
    if package.get('dependencies', {}).get(dependency) != version:
        raise SystemExit(f'G03 dependency {dependency} must be pinned exactly to {version}')
if package.get('devDependencies', {}).get('@playwright/test') != '1.62.1':
    raise SystemExit('G03 browser tests must pin @playwright/test exactly to 1.62.1')

for migration in (ROOT / 'apps/desktop/src-tauri/migrations').glob('*.sql'):
    migration_text = migration.read_text(encoding='utf-8')
    if re.search(r'^\s*[A-Za-z_][A-Za-z0-9_]*\s+BLOB\b', migration_text, re.IGNORECASE | re.MULTILINE):
        raise SystemExit(f'Metadata-only migration contains a BLOB column: {migration.relative_to(ROOT)}')

plan_migration = (ROOT / 'apps/desktop/src-tauri/migrations/0004_job_operation_plans.sql').read_text(encoding='utf-8')
if 'length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536' not in plan_migration:
    raise SystemExit('G03 plan migration is missing the approved exact UTF-8 byte-length constraint')


def validate_g03_acceptance_consistency(inputs: dict[str, object]) -> None:
    state = str(inputs['state']).lower()
    for stale in [
        'g03 ready to stage',
        'pre-stage acceptance audit',
        'no g03 changes are staged or published',
        'no staging, commit, push, pull request',
        'no commit, push or pr',
    ]:
        if stale in state:
            raise SystemExit(f'Stale G03 repository-state claim remains: {stale}')
    for false_status in [
        'g03 is not complete',
        'g03 is not merged',
        'g04 remains blocked',
        'g04 is blocked',
        'draft pr #6',
    ]:
        if false_status in state:
            raise SystemExit(f'Stale G03/G04 status claim remains: {false_status}')
    for required in [
        'g03 — complete',
        '8d6844ebdc1fd6eedf41373d53ad36eb399cc489',
        'g04a — complete',
        'a27306653119e6e4fcdef162308445b78129f974',
        'g04b — complete',
        '6940a1381822a5872f4c345cc0e5cd15b2e6294c',
        'g04b2 — active implementation',
        'g04b2 is not accepted or complete',
    ]:
        if required not in state:
            raise SystemExit(f'Current G03/G04 status is missing required truth: {required}')

    manifest = inputs['asset_manifest']
    if not isinstance(manifest, dict) or manifest.get('version') != '6.2.108':
        raise SystemExit('G03 exact PDF.js asset manifest is missing or has the wrong version')
    files = manifest.get('files')
    if not isinstance(files, list) or len(files) != 191:
        raise SystemExit('G03 exact PDF.js manifest must contain the reviewed 191-file allow-list')
    paths = [entry.get('path') for entry in files if isinstance(entry, dict)]
    if len(paths) != len(set(paths)) or 'pdf.worker.mjs' not in paths:
        raise SystemExit('G03 PDF.js manifest paths must be unique and include the worker')
    staging = str(inputs['staging'])
    verifier = str(inputs['verifier'])
    for required in ['verifyStagedDirectory', 'faultAfterBackup', 'rename(temporary, outputRoot)']:
        if required not in staging:
            raise SystemExit(f'G03 atomic exact staging is missing {required}')
    for required in [
        'Compare-Object $expectedPdfPaths $actualPdfPaths',
        'Get-Sha256File',
        '[System.Security.Cryptography.SHA256]::Create()',
        'pdfjs-assets-6.2.108.json',
    ]:
        if required not in verifier:
            raise SystemExit(f'G03 independent PDF.js verifier is missing {required}')

    viewer = str(inputs['viewer'])
    surface = str(inputs['surface'])
    viewer_tests = str(inputs['viewer_tests'])
    if 'all virtualizer items are visible' in state or 'all virtual items are visible pages' in state:
        raise SystemExit('G03 documentation conflates mounted overscan with visible pages')
    for required in ['visiblePageMetrics', 'intersectionArea', 'data-visible', 'actualVisiblePages']:
        if required not in viewer:
            raise SystemExit(f'G03 true viewport visibility implementation is missing {required}')
    for required in ['MAX_CANVAS_PIXELS', 'MAX_CANVAS_WIDTH', 'MAX_CANVAS_HEIGHT', 'safeCanvasAllocation']:
        if required not in surface and required not in str(inputs['session']):
            raise SystemExit(f'G03 fail-closed canvas boundary is missing {required}')
    if '16_777_216' not in str(inputs['session']) or '8_192' not in str(inputs['session']):
        raise SystemExit('G03 canonical canvas limits changed or are absent')
    for required in ['event.altKey', 'onReorder', 'reorderSelectedPages']:
        if required not in viewer + surface:
            raise SystemExit(f'G03 Alt+Arrow organizer implementation is missing {required}')
    if 'Alt+Arrow block reorder' not in viewer_tests:
        raise SystemExit('G03 Alt+Arrow browser regression is missing')
    for required in ['candidateDocumentRef', 'disposeOwnedDocument', 'previousActive']:
        if required not in viewer:
            raise SystemExit(f'G03 transactional candidate replacement is missing {required}')
    runtime = str(inputs['runtime'])
    environment_policy = str(inputs['environment_policy'])
    if runtime.index('webview2_environment::enforce_webview2_environment_policy();') > runtime.index(
        'let context = tauri::generate_context!();'
    ):
        raise SystemExit('SEC1C WebView2 environment policy runs after Tauri context generation')
    for required in [
        'WEBVIEW2_ENVIRONMENT_PREFIX',
        'COREWEBVIEW2_MAX_INSTANCES_ENV',
        'COREWEBVIEW2_MAX_INSTANCES_VALUE: &str = "20"',
        'std::env::vars_os()',
        'eq_ignore_ascii_case',
        'std::env::remove_var(key)',
        'std::env::set_var(plan.enforced_key, plan.enforced_value)',
        'test_runtime_environment_evidence',
        'WeBvIeW2_FuTuRe_HOSTILE_OVERRIDE',
        'CoReWeBvIeW2_MaX_InStAnCeS',
    ]:
        if required not in environment_policy:
            raise SystemExit(f'SEC1C WebView2 environment policy is missing {required}')
    if 'renderForms: false' not in str(inputs['session']) or 'AnnotationMode.DISABLE' not in str(inputs['session']):
        raise SystemExit('G03 form/annotation rendering boundary is not explicit')
    session = str(inputs['session'])
    contracts = str(inputs['contracts'])
    rust_contracts = str(inputs['rust_contracts'])
    rust_pdf = str(inputs['rust_pdf'])
    rust_viewer = str(inputs['rust_viewer'])
    session_tests = str(inputs['session_tests'])
    for required in [
        'CORE_PDF_MAX_PAGES = 4096',
        'validatePdfPageCount(document.numPages);',
        "code: 'PDF_PAGE_COUNT_UNSUPPORTED'",
    ]:
        if required not in contracts + session:
            raise SystemExit(f'G03 PDF.js page-count admission boundary is missing {required}')
    if 'CORE_PDF_MAX_PAGES: u32 = 4096' not in rust_contracts:
        raise SystemExit('G03 Rust page-count constant differs from the TypeScript contract')
    if 'CORE_PDF_MAX_PAGES + 1' not in session_tests:
        raise SystemExit('G03 PDF.js page-count rejection regression is missing')
    for required in [
        'page_count == 0 || page_count > u64::from(CORE_PDF_MAX_PAGES)',
        '.try_reserve_exact(page_count)',
        'qpdf_page_count_rejects_unsupported_and_malicious_numbers',
    ]:
        if required not in rust_pdf:
            raise SystemExit(f'G03 qpdf page-count allocation boundary is missing {required}')
    for required in [
        'RANGE_CHUNK_BYTES = 256 * 1024',
        'MAX_RANGE_READS = 4',
        'MAX_QUEUED_RANGE_COUNT = 64',
        'MAX_QUEUED_RANGE_BYTES = 16 * 1024 * 1024',
    ]:
        if required not in contracts:
            raise SystemExit(f'G03 PDF range queue contract drifted: {required}')
    if 'VIEWER_RANGE_CHUNK_BYTES: u64 = 256 * 1024' not in rust_viewer:
        raise SystemExit('G03 Rust viewer range chunk differs from the TypeScript contract')
    for required in [
        'PDF_RANGE_QUEUE_LIMIT_EXCEEDED',
        'checkedQueueTotal',
        'transportEpoch',
        'references += 1',
        'flushCompletedLogicalRanges',
    ]:
        if required not in session:
            raise SystemExit(f'G03 PDF range queue boundary is missing {required}')
    for required in [
        'rejects count plus one',
        'rejects bytes plus one',
        'replacement document',
        'FIFO progress',
        'releases logical count and byte accounting',
        'sanitizes native range failures',
    ]:
        if required not in session_tests:
            raise SystemExit(f'G03 PDF range queue regression is missing {required}')


g03_state_paths = [
    ROOT / 'MANIFEST.json',
    ROOT / 'README.md',
    ROOT / 'docs/18-ROADMAP-AND-MILESTONES.md',
]
G03_VALIDATION_INPUTS = {
    'state': '\n'.join(path.read_text(encoding='utf-8') for path in g03_state_paths),
    'asset_manifest': json.loads((ROOT / 'apps/desktop/scripts/pdfjs-assets-6.2.108.json').read_text(encoding='utf-8')),
    'staging': (ROOT / 'apps/desktop/scripts/stage-pdfjs-assets.mjs').read_text(encoding='utf-8'),
    'verifier': (ROOT / 'apps/desktop/scripts/verify-g03-boundaries.ps1').read_text(encoding='utf-8'),
    'viewer': (ROOT / 'apps/desktop/src/viewer/ViewerWorkspace.tsx').read_text(encoding='utf-8'),
    'surface': (ROOT / 'apps/desktop/src/viewer/PageSurface.tsx').read_text(encoding='utf-8'),
    'session': (ROOT / 'apps/desktop/src/viewer/pdfSession.ts').read_text(encoding='utf-8'),
    'viewer_tests': (ROOT / 'apps/desktop/e2e/viewer.spec.ts').read_text(encoding='utf-8'),
    'runtime': (ROOT / 'apps/desktop/src-tauri/src/lib.rs').read_text(encoding='utf-8'),
    'environment_policy': (ROOT / 'apps/desktop/src-tauri/src/webview2_environment.rs').read_text(encoding='utf-8'),
    'contracts': (ROOT / 'packages/contracts/src/index.ts').read_text(encoding='utf-8'),
    'rust_contracts': (ROOT / 'apps/desktop/src-tauri/src/contracts.rs').read_text(encoding='utf-8'),
    'rust_pdf': (ROOT / 'apps/desktop/src-tauri/src/pdf_merge.rs').read_text(encoding='utf-8'),
    'rust_viewer': (ROOT / 'apps/desktop/src-tauri/src/viewer_sessions.rs').read_text(encoding='utf-8'),
    'session_tests': (ROOT / 'apps/desktop/src/viewer/pdfSession.test.ts').read_text(encoding='utf-8'),
}
validate_g03_acceptance_consistency(G03_VALIDATION_INPUTS)

g04b_spec_migration = (
    ROOT / 'apps/desktop/src-tauri/migrations/0005_job_operation_specs_and_warnings.sql'
).read_text(encoding='utf-8')
for required_constraint in [
    'CREATE TABLE job_operation_specs',
    'length(CAST(settings_json AS BLOB)) BETWEEN 2 AND 65536',
    'CREATE TABLE job_warnings',
    'sanitized_detail TEXT NOT NULL',
]:
    if required_constraint not in g04b_spec_migration:
        raise SystemExit(f'G04B metadata migration is missing {required_constraint}')


def validate_g04b_boundaries(inputs: dict[str, str]) -> None:
    cargo = inputs['cargo']
    for required in [
        'flate2 = { version = "=1.1.9", default-features = false, features = ["rust_backend"] }',
        'image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }',
        'pdf-writer = { version = "=0.15.0", default-features = false }',
    ]:
        if required not in cargo:
            raise SystemExit(f'G04B exact dependency declaration is missing: {required}')
    for prohibited in ['libvips', 'pdfium', 'mupdf', 'poppler', 'ghostscript', 'reqwest']:
        if re.search(rf'^\s*{prohibited}\s*=', cargo, re.IGNORECASE | re.MULTILINE):
            raise SystemExit(f'G04B unreviewed dependency found: {prohibited}')

    contracts = inputs['contracts']
    for required in [
        'IMAGE_TO_PDF_OPERATION_ID: &str = "image.to-pdf"',
        'IMAGE_TO_PDF_MAX_INPUTS: usize = 128',
        'IMAGE_MAX_DIMENSION: u32 = 8_192',
        'IMAGE_MAX_PIXELS: u64 = 16_777_216',
        'IMAGE_TO_PDF_MAX_TOTAL_PIXELS: u64 = 67_108_864',
        'IMAGE_TO_PDF_MAX_TOTAL_INPUT_BYTES: u64 = 536_870_912',
    ]:
        if required not in contracts:
            raise SystemExit(f'G04B bounded contract is missing: {required}')

    writer = inputs['writer']
    for required in [
        'apply_orientation',
        's_mask',
        'verify_output',
        'source_hashes',
        'publish_verified_staging_with_observer',
        'record_warning',
    ]:
        if required not in writer:
            raise SystemExit(f'G04B writer/verifier path is missing: {required}')

    image_tests = inputs['image_tests']
    for required in [
        'altered_persisted_settings_fail_closed_before_conversion',
        'let too_many = vec![source.clone(); 129]',
        'let maximum = vec![source.clone(); 128]',
        'IMAGE_PDF_PUBLICATION_FAILED',
        'reconcile_startup',
    ]:
        if required not in image_tests:
            raise SystemExit(f'G04B native acceptance evidence is missing: {required}')

    viewer_tests = inputs['viewer_tests']
    if 'G04B images-to-PDF output matches its source pixels through the accepted PDF.js renderer' not in viewer_tests:
        raise SystemExit('G04B browser-backed visual evidence is missing')
    if '"pretest:browser": "node ./scripts/prepare-g04b-visual-evidence.mjs"' not in inputs['package']:
        raise SystemExit('G04B native visual producer must run before Playwright timing')
    for required in [
        'DOCUMENT_STUDIO_G04B_VISUAL_EVIDENCE_DIR',
        'jpeg_png_and_webp_publish_one_verified_page_each_in_selected_order',
        "'--', '--exact'",
        "'source.png', 'output.pdf'",
    ]:
        if required not in inputs['visual_producer']:
            raise SystemExit(f'G04B pre-browser visual producer is missing: {required}')
    if "'target', 'g04b-browser-visual-evidence'" not in viewer_tests:
        raise SystemExit('G04B browser test is not consuming pre-generated native evidence')

    convert = inputs['convert']
    for required in [
        'PDF to images',
        'event.altKey',
        "event.key === 'Delete'",
        'api.jobs.warnings',
    ]:
        if required not in convert:
            raise SystemExit(f'G04B truthful accessible UI is missing: {required}')
G04B_VALIDATION_INPUTS = {
    'cargo': (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text(encoding='utf-8'),
    'contracts': (ROOT / 'apps/desktop/src-tauri/src/contracts.rs').read_text(encoding='utf-8'),
    'writer': (ROOT / 'apps/desktop/src-tauri/src/image_to_pdf.rs').read_text(encoding='utf-8'),
    'image_tests': (ROOT / 'apps/desktop/src-tauri/tests/image_to_pdf.rs').read_text(encoding='utf-8'),
    'viewer_tests': (ROOT / 'apps/desktop/e2e/viewer.spec.ts').read_text(encoding='utf-8'),
    'package': (ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8'),
    'visual_producer': (ROOT / 'apps/desktop/scripts/prepare-g04b-visual-evidence.mjs').read_text(encoding='utf-8'),
    'convert': (ROOT / 'apps/desktop/src/ConvertWorkspace.tsx').read_text(encoding='utf-8'),
}
validate_g04b_boundaries(G04B_VALIDATION_INPUTS)


def validate_g04b2_boundaries(inputs: dict[str, str]) -> None:
    cargo = inputs['cargo']
    package = json.loads(inputs['package'])
    if 'image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }' not in cargo:
        raise SystemExit('G04B2 must use the accepted narrow image 0.25.10 encoder')
    if package.get('dependencies', {}).get('pdfjs-dist') != '6.2.108':
        raise SystemExit('G04B2 must use exact accepted pdfjs-dist 6.2.108')
    if 'verify:g04b2' not in package.get('scripts', {}):
        raise SystemExit('G04B2 boundary verifier script is not registered')
    ci = inputs['ci']
    for required in [
        'npm run verify:g04b2 --workspace @document-studio/desktop',
        'npm run test:webview2 --workspace @document-studio/desktop',
    ]:
        if required not in ci:
            raise SystemExit(f'G04B2 exact-head CI is missing {required}')
    manifests = cargo + '\n' + inputs['package']
    for prohibited in ['pdfium', 'mupdf', 'poppler', 'pdfbox', 'ghostscript', 'reqwest']:
        if re.search(rf'(^|["\s]){prohibited}(["\s=:]|$)', manifests, re.IGNORECASE | re.MULTILINE):
            raise SystemExit(f'G04B2 contains forbidden renderer/network dependency {prohibited}')
    capability = json.loads(inputs['capability'])
    if capability.get('permissions') != ['core:default', 'dialog:allow-open']:
        raise SystemExit('G04B2 changed the accepted minimum Tauri capability set')

    contracts = inputs['contracts']
    for required in [
        'PDF_TO_IMAGES_OPERATION_ID: &str = "pdf.to-images"',
        'PDF_TO_IMAGES_VERSION: &str = "1.0.0"',
        'PDF_TO_IMAGES_MAX_OUTPUTS: usize = 128',
        'PDF_TO_IMAGES_MAX_TOTAL_PIXELS: u64 = 67_108_864',
        'PDF_TO_IMAGES_JPEG_QUALITY: u8 = 92',
        'PDFJS_VERSION: &str = "6.2.108"',
        'IMAGE_MAX_DIMENSION: u32 = 8_192',
        'IMAGE_MAX_PIXELS: u64 = 16_777_216',
    ]:
        if required not in contracts:
            raise SystemExit(f'G04B2 contract boundary is missing: {required}')
    typed = inputs['typed_contracts']
    request_block = typed.split('export interface PdfToImagesJobCreateRequest', 1)[1].split('}', 1)[0]
    for required in ['viewerSessionId: string', 'destinationGrantId: string', 'dpi: 72 | 150 | 300']:
        if required not in request_block:
            raise SystemExit(f'G04B2 opaque typed request is missing {required}')
    if re.search(r'\b(sourcePath|destinationPath|destinationDirectory)\b', request_block):
        raise SystemExit('G04B2 typed request exposes a source/destination path to React')

    registry = inputs['registry']
    for required in [
        'pub fn pdf_to_images_manifest()',
        'PDF_TO_IMAGES_OPERATION_ID',
        'PDF_TO_IMAGES_VERSION',
        '"format": { "enum": ["jpeg", "png", "webp"] }',
        '"dpi": { "enum": [72, 150, 300] }',
        'value.outputs.multiplicity = "multiple"',
        '"authenticated-binary-ipc"',
    ]:
        if required not in registry:
            raise SystemExit(f'G04B2 operation registry is missing {required}')

    session = inputs['session']
    for required in [
        'isEvalSupported: false', 'enableXfa: false', 'renderForms: false',
        'AnnotationMode.DISABLE', 'disableAutoFetch: true', 'useSystemFonts: false',
    ]:
        if required not in session:
            raise SystemExit(f'G04B2 accepted PDF.js security setting is missing {required}')

    renderer = inputs['renderer']
    for required in [
        'dpi / 72', 'PDF_TO_IMAGES_MAX_OUTPUTS = 128',
        'PDF_TO_IMAGES_MAX_TOTAL_PIXELS = 67_108_864',
        "window.document.createElement('canvas')", "alpha: false",
        "background: '#ffffff'", "intent: 'print'", 'getImageData',
        'submitPdfPixels', 'renderTask?.cancel()', 'page.cleanup()',
        'canvas.width = 0', 'canvas.height = 0',
    ]:
        if required not in renderer:
            raise SystemExit(f'G04B2 sequential renderer is missing {required}')
    for prohibited in ['toBlob(', 'convertToBlob(', 'Promise.all(', 'fetch(', 'http://', 'https://']:
        if prohibited in renderer:
            raise SystemExit(f'G04B2 renderer contains prohibited path {prohibited}')

    api = inputs['api']
    for required in [
        "invoke<JobRecord>('pdf_to_images_submit_page', rgba, {",
        "'x-document-studio-job-id'", "'x-document-studio-render-session-id'",
        "'x-document-studio-page-ordinal'", "'x-document-studio-page-nonce'",
        "'x-document-studio-expected-width'", "'x-document-studio-expected-height'",
    ]:
        if required not in api:
            raise SystemExit(f'G04B2 raw client IPC is missing {required}')
    if 'base64' in api[api.find('submitPdfPixels'):api.find('cancel:', api.find('submitPdfPixels'))]:
        raise SystemExit('G04B2 raw client IPC uses base64')

    ipc = inputs['ipc']
    for required in [
        "request: Request<'_>", 'InvokeBody::Raw(bytes)', 'bytes.len() <= 67_108_864',
        'spawn_blocking', 'pixel_upload_metadata(&request)',
        'x-document-studio-job-id', 'x-document-studio-render-session-id',
        'x-document-studio-page-ordinal', 'x-document-studio-page-nonce',
        'x-document-studio-expected-width', 'x-document-studio-expected-height',
        'PDF_TO_IMAGES_OPERATION_ID', 'redact_viewer_job_paths',
        'tauri::webview_version().ok()',
    ]:
        if required not in ipc:
            raise SystemExit(f'G04B2 raw Rust IPC boundary is missing {required}')

    backend = inputs['backend']
    for required in [
        'metadata.render_session_id != active.render_session_id',
        'metadata.page_ordinal as usize != expected_ordinal',
        'metadata.nonce != page.ticket.nonce',
        'metadata.expected_width != page.ticket.expected_width',
        'metadata.expected_height != page.ticket.expected_height',
        'rgba.len() != expected_bytes', 'pixel[3] != 255',
        'checked_mul(u64::from(metadata.expected_height))',
        'pixels.checked_mul(4)', 'check_cancelled',
        'PngEncoder::new_with_quality', 'JpegEncoder::new_with_quality',
        'PDF_TO_IMAGES_JPEG_QUALITY', 'WebPEncoder::new_lossless',
        'insert_png_density', 'PixelDensity::dpi', 'mean_absolute_error',
        'decoded.color().has_alpha()',
        'verify_exact_staging_membership', 'verify_unchanged_hash',
        'verify_all_staging_hashes', 'size != verified_size',
        'publish_verified_staging_with_observer', 'PARTIAL_PUBLICATION',
    ]:
        if required not in backend:
            raise SystemExit(f'G04B2 Rust service boundary is missing {required}')

    workspace = inputs['workspace']
    for required in [
        'PageThumbnail', "(['jpeg', 'png', 'webp'] as const)",
        '([72, 150, 300] as const)', 'Select no more than 128 pages',
        'Partial publication', 'RenderTask', 'transport.abort()',
        'openButtonRef.current?.focus()', 'source path stays outside React',
    ]:
        if required not in workspace:
            raise SystemExit(f'G04B2 accessible UI boundary is missing {required}')

    native_tests = inputs['native_tests']
    for required in [
        'png_jpeg_and_lossless_webp_are_encoded_verified_and_published',
        'png_and_jpeg_record_each_supported_density',
        'stale_wrong_page_replay_and_payload_mismatch_fail_closed',
        'exact_128_output_boundary_is_accepted_and_129_is_rejected',
        'dimension_pixel_and_aggregate_caps_are_enforced_before_session_access',
        'malformed_and_encrypted_pdfs_are_rejected_before_rendering',
        'cancellation_cleans_owned_staging_and_source_remains_immutable',
        'partial_publication_preserves_the_first_user_file_and_reconciles_the_rest',
        'verified_staging_hash_drift_fails_before_any_publication',
        'measure_maximum_page_encode_cancellation_response',
        'unexpected_staging_membership_fails_before_any_publication',
        'startup_recovery_fails_an_interrupted_render_job_and_removes_private_workspace',
        'destination_collision_never_overwrites_the_racing_user_file',
        'PIXEL_ALPHA_INVALID',
    ]:
        if required not in native_tests:
            raise SystemExit(f'G04B2 native acceptance evidence is missing {required}')
    browser = inputs['browser']
    for required in [
        'G04B2 PDF-to-images renders ordered opaque pages sequentially through authenticated binary transfer',
        'G04B2 PDF-to-images covers rotated crop boxes, embedded Type 3, soft masks, and ICC CMYK Lab rendering',
        'peakPdfImageTransfers', 'alphaOpaque', 'nonWhitePixels',
    ]:
        if required not in browser:
            raise SystemExit(f'G04B2 browser evidence is missing {required}')
    webview_smoke = inputs['webview_smoke']
    for required in [
        "'viewer_grant_test_destination'", "'pdf_to_images_submit_page'",
        'missingHeaderRejected', 'replayRejected', 'jsonPixelBodyRejected',
        'webview2RuntimeVersion', 'webview2-g04b2-page-0001.png',
    ]:
        if required not in webview_smoke:
            raise SystemExit(f'G04B2 native WebView2 evidence is missing {required}')
    if not re.search(r'#\[cfg\(feature = "test-runtime"\)\]\s+#\[tauri::command\]\s+pub fn viewer_grant_test_destination', ipc):
        raise SystemExit('G04B2 native smoke destination grant is not test-runtime gated')
    adr = inputs['adr']
    for required in [
        'pdfjs-dist` 6.2.108', 'private, unattached canvas',
        'binary Tauri IPC', 'WebView2 runtime version',
        'does not promise byte-identical pixels', 'getImageData()` is synchronous',
    ]:
        if required not in adr:
            raise SystemExit(f'G04B2 ADR is missing {required}')


G04B2_VALIDATION_INPUTS = {
    'cargo': (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text(encoding='utf-8'),
    'package': (ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8'),
    'ci': (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8'),
    'capability': (ROOT / 'apps/desktop/src-tauri/capabilities/default.json').read_text(encoding='utf-8'),
    'contracts': (ROOT / 'apps/desktop/src-tauri/src/contracts.rs').read_text(encoding='utf-8'),
    'typed_contracts': (ROOT / 'packages/contracts/src/index.ts').read_text(encoding='utf-8'),
    'registry': (ROOT / 'apps/desktop/src-tauri/src/operation_registry.rs').read_text(encoding='utf-8'),
    'session': (ROOT / 'apps/desktop/src/viewer/pdfSession.ts').read_text(encoding='utf-8'),
    'renderer': (ROOT / 'apps/desktop/src/viewer/pdfToImages.ts').read_text(encoding='utf-8'),
    'api': (ROOT / 'apps/desktop/src/api.ts').read_text(encoding='utf-8'),
    'ipc': (ROOT / 'apps/desktop/src-tauri/src/ipc.rs').read_text(encoding='utf-8'),
    'backend': (ROOT / 'apps/desktop/src-tauri/src/pdf_to_images.rs').read_text(encoding='utf-8'),
    'workspace': (ROOT / 'apps/desktop/src/PdfToImagesWorkspace.tsx').read_text(encoding='utf-8'),
    'native_tests': (ROOT / 'apps/desktop/src-tauri/tests/pdf_to_images.rs').read_text(encoding='utf-8'),
    'browser': (ROOT / 'apps/desktop/e2e/viewer.spec.ts').read_text(encoding='utf-8'),
    'webview_smoke': (ROOT / 'apps/desktop/scripts/webview2-smoke.mjs').read_text(encoding='utf-8'),
    'adr': (ROOT / 'docs/adr/ADR-013-g04b-image-pdf-conversion-dependencies.md').read_text(encoding='utf-8'),
}
validate_g04b2_boundaries(G04B2_VALIDATION_INPUTS)

legacy_name = 'Rohith' + ' Document Studio'
for p in ROOT.rglob('*.md'):
    if 'attachments/archive' in str(p):
        continue
    if legacy_name in p.read_text(encoding='utf-8', errors='ignore'):
        raise SystemExit(f'Legacy name found outside archive: {p.relative_to(ROOT)}')

print(
    'Repository validation passed. '
    f'{len(rows)} feature entries found; G01-G04B accepted status and G04B2 contract, raw IPC, scope and document consistency verified.'
)
