from pathlib import Path
import csv
import json
import re
import yaml

from g04d_c_powershell_source_policy import (
    format_violations,
    validate_g04dc_powershell_source_bytes,
)

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
    'packages/contracts/batch.schema.json',
    'packages/contracts/package.json',
    'packages/contracts/fixtures/foundation-contracts.json',
    'packages/contracts/fixtures/batch-preview-contracts.json',
    'packages/contracts/fixtures/text-to-pdf-contracts.json',
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
    'apps/desktop/src-tauri/migrations/0006_job_completion_outcomes.sql',
    'apps/desktop/src-tauri/migrations/0007_balanced_compression_audits.sql',
    'apps/desktop/src-tauri/migrations/0008_batch_preview_foundation.sql',
    'apps/desktop/src-tauri/resources/fonts/g04e1/font-manifest.json',
    'apps/desktop/src-tauri/resources/fonts/g04e1/NotoSans-Regular.ttf',
    'apps/desktop/src-tauri/resources/fonts/g04e1/NotoSansDevanagari-Regular.ttf',
    'apps/desktop/src-tauri/resources/fonts/g04e1/NotoSansTelugu-Regular.ttf',
    'apps/desktop/src-tauri/resources/fonts/g04e1/licenses/NotoSans-OFL-1.1.txt',
    'apps/desktop/src-tauri/resources/fonts/g04e1/licenses/NotoSansDevanagari-OFL-1.1.txt',
    'apps/desktop/src-tauri/resources/fonts/g04e1/licenses/NotoSansTelugu-OFL-1.1.txt',
    'apps/desktop/src-tauri/src/text_to_pdf.rs',
    'apps/desktop/src-tauri/src/text_to_pdf_renderer.rs',
    'apps/desktop/src-tauri/src/text_to_pdf_service.rs',
    'apps/desktop/src-tauri/src/webview2_g04e1_compile_gate.rs',
    'apps/desktop/src/TextToPdfWorkspace.tsx',
    'apps/desktop/src/TextToPdfWorkspace.test.tsx',
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
    'docs/implementation-log/G04C2-corpus-recovery.md',
    'docs/adr/ADR-017-canonical-batch-preview-and-atomic-metadata.md',
    'docs/adr/ADR-018-non-mutating-libreoffice-runtime-acquisition.md',
    'docs/implementation-log/G04F1-batch-preview-foundation.md',
    'docs/adr/ADR-019-hidden-webview2-text-pdf-renderer.md',
    'docs/implementation-log/G04E1-txt-to-pdf.md',
    'docs/implementation-log/G04D-C-libreoffice-runtime-admission.md',
    '.github/workflows/g04d-c-libreoffice-runtime-proof.yml',
    'scripts/g04d-c/G04DC.Common.psm1',
    'scripts/g04d-c/ClassRegistryDigest.cs',
    'scripts/g04d-c/G04DC.Sandbox.cs',
    'scripts/g04d-c/G04DC.MsiCondition.cs',
    'scripts/g04d-c/Invoke-G04DCAdminImageProof.ps1',
    'scripts/g04d-c/Invoke-G04DCMachineStatePrecheck.ps1',
    'scripts/g04d-c/Invoke-G04DCLocalMsiReadOnlyValidation.ps1',
    'scripts/g04d-c/Invoke-G04DCMinimalMsiProof.ps1',
    'scripts/g04d-c/Invoke-G04DCSandboxSmoke.ps1',
    'scripts/g04d-c/New-G04DCCandidateDecision.ps1',
    'scripts/g04d-c/New-G04DCRuntimeManifest.ps1',
    'scripts/g04d-c/New-G04DCSyntheticOdt.ps1',
    'scripts/g04d-c/Test-G04DCBoundaries.ps1',
    'scripts/g04d-c/Test-G04DCPowerShell51Source.ps1',
    'scripts/g04d-c/verify-g04d-c-boundaries.ps1',
    'scripts/g04d-c/verify-pdfjs.mjs',
    'scripts/g04d_c_powershell_source_policy.py',
    'scripts/g04c2_corpus.py',
    'apps/desktop/scripts/verify-g04c2-corpus.ps1',
    'apps/desktop/scripts/verify-g04c2b-boundaries.ps1',
    'apps/desktop/scripts/verify-g04f1-boundaries.ps1',
    'apps/desktop/scripts/verify-g04e1-boundaries.ps1',
    'apps/desktop/scripts/prepare-g04e1-visual-evidence.mjs',
    'packages/contracts/fixtures/pdf-compress-balanced-contracts.json',
    'apps/desktop/src-tauri/tests/fixtures/g04c2-balanced-corpus/README.md',
    'apps/desktop/src-tauri/tests/fixtures/g04c2-balanced-corpus/corpus-manifest.json',
    'apps/desktop/src-tauri/tests/g04c2_corpus.rs',
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

g04dc_source_report = validate_g04dc_powershell_source_bytes(
    ROOT,
    ROOT / 'scripts/g04d-c',
)
if g04dc_source_report['violations']:
    raise SystemExit(
        'G04D-C executable source byte violation: '
        + format_violations(g04dc_source_report)
    )

for p in [
    'MANIFEST.json',
    'apps/desktop/package.json',
    'apps/desktop/src-tauri/tauri.conf.json',
    'apps/desktop/src-tauri/resources/fonts/g04e1/font-manifest.json',
    'apps/desktop/src-tauri/capabilities/default.json',
    'package.json',
    'package-lock.json',
    'packages/contracts/ipc.schema.json',
    'packages/contracts/batch.schema.json',
    'packages/contracts/fixtures/foundation-contracts.json',
    'packages/contracts/fixtures/batch-preview-contracts.json',
    'packages/contracts/fixtures/text-to-pdf-contracts.json',
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
        'g04b2 — active implementation',
        'g04b2 is not accepted or complete',
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
        'accepted on main at merge b5901a7baca58b3acb1ee00027e42b0059c59fd4',
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
    if '"pretest:browser": "node ./scripts/prepare-g04b-visual-evidence.mjs && node ./scripts/prepare-g04e1-visual-evidence.mjs"' not in inputs['package']:
        raise SystemExit('G04B native visual producer must run before Playwright timing')
    for required in [
        'DOCUMENT_STUDIO_G04B_VISUAL_EVIDENCE_DIR',
        'jpeg_png_and_webp_publish_one_verified_page_each_in_selected_order',
        "'--', '--exact'",
        "'source.png', 'output.pdf'",
    ]:
        if required not in inputs['visual_producer']:
            raise SystemExit(f'G04B pre-browser visual producer is missing: {required}')
    for required in [
        'DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT',
        "resolve(browserEvidenceRoot, 'g04b-browser-visual-evidence')",
    ]:
        if required not in viewer_tests:
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

g04c2_manifest = json.loads(
    (ROOT / 'apps/desktop/src-tauri/tests/fixtures/g04c2-balanced-corpus/corpus-manifest.json').read_text(encoding='utf-8')
)
if g04c2_manifest.get('CORPUS_MODE') != 'reviewed-rebaseline':
    raise SystemExit('G04C2 corpus mode must preserve the independently reviewed rebaseline')
if len(g04c2_manifest.get('entries', [])) != 6 or len(g04c2_manifest.get('generatedPdfs', [])) != 7:
    raise SystemExit('G04C2 corpus must contain exactly six JPEG entries and seven generated PDFs')
uzh_entries = [entry for entry in g04c2_manifest['entries'] if entry.get('id') == 'uzh-river']
if len(uzh_entries) != 1 or uzh_entries[0].get('previousFrozenEvidence') != {
    'dimensions': {'width': 1280, 'height': 817},
    'bytes': 296546,
    'sha256': '0fa88acf594e48c5a8e87e588056f66aad4cc00035b655648c37c1b54938e727',
}:
    raise SystemExit('G04C2 Uzh previous frozen evidence was not preserved exactly')
g04c2_script = (ROOT / 'scripts/g04c2_corpus.py').read_text(encoding='utf-8')
for prohibited in ['urllib', 'requests.', 'Invoke-WebRequest', 'Invoke-RestMethod']:
    if prohibited in g04c2_script:
        raise SystemExit(f'G04C2 CI corpus validator contains a network path: {prohibited}')
desktop_scripts = json.loads((ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8')).get('scripts', {})
if 'verify:g04c2-corpus' not in desktop_scripts:
    raise SystemExit('G04C2 corpus verifier is not registered in the desktop package')
ci_text = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
for required_ci in [
    'python -B scripts/g04c2_corpus.py check',
    'npm run verify:g04c2-corpus --workspace @document-studio/desktop',
]:
    if required_ci not in ci_text:
        raise SystemExit(f'G04C2 corpus exact-head CI is missing: {required_ci}')

g04c2b_contracts = (ROOT / 'apps/desktop/src-tauri/src/contracts.rs').read_text(encoding='utf-8')
g04c2b_backend = (ROOT / 'apps/desktop/src-tauri/src/balanced_compression.rs').read_text(encoding='utf-8')
g04c2b_metrics = (ROOT / 'apps/desktop/src-tauri/src/balanced_metrics.rs').read_text(encoding='utf-8')
g04c2b_migration = (ROOT / 'apps/desktop/src-tauri/migrations/0007_balanced_compression_audits.sql').read_text(encoding='utf-8')
for required_boundary in [
    'BALANCED_COMPRESSION_OPERATION_ID: &str = "pdf.compress-balanced"',
    'BALANCED_COMPRESSION_PROFILE: &str = "balanced-v1"',
    'BALANCED_COMPRESSION_JPEG_QUALITY: u8 = 82',
    'BALANCED_COMPRESSION_MAX_AFFECTED_PAGES: usize = 128',
]:
    if required_boundary not in g04c2b_contracts:
        raise SystemExit(f'G04C2B fixed contract is missing: {required_boundary}')
for required_boundary in [
    'OsString::from("--stream-data=preserve")',
    'OsString::from("--object-streams=preserve")',
    'OsString::from("--preserve-unreferenced")',
    'document_savings_gate_passes(active.source_size, active.candidate_size)',
    'frozen_corpus_completes_no_benefit_without_visual_or_output',
]:
    if required_boundary not in g04c2b_backend:
        raise SystemExit(f'G04C2B backend boundary is missing: {required_boundary}')
for required_boundary in [
    'BALANCED_SSIM_MINIMUM: f64 = 0.985',
    'BALANCED_PSNR_MINIMUM_DB: f64 = 36.0',
    'BALANCED_CHANGED_DELTA_THRESHOLD: u8 = 12',
]:
    if required_boundary not in g04c2b_metrics:
        raise SystemExit(f'G04C2B exact metric is missing: {required_boundary}')
if ') STRICT;' not in g04c2b_migration or "CHECK (profile = 'balanced-v1')" not in g04c2b_migration:
    raise SystemExit('G04C2B audit migration must remain strict and profile-bound')
if 'npm run verify:g04c2b --workspace @document-studio/desktop' not in ci_text:
    raise SystemExit('G04C2B boundary verifier is not wired into exact-head CI')

g04f1_batch = (ROOT / 'apps/desktop/src-tauri/src/batch.rs').read_text(encoding='utf-8')
g04f1_database = (ROOT / 'apps/desktop/src-tauri/src/database.rs').read_text(encoding='utf-8')
g04f1_migration = (ROOT / 'apps/desktop/src-tauri/migrations/0008_batch_preview_foundation.sql').read_text(encoding='utf-8')
g04f1_schema = json.loads((ROOT / 'packages/contracts/batch.schema.json').read_text(encoding='utf-8'))
g04f1_ipc_schema = json.loads((ROOT / 'packages/contracts/ipc.schema.json').read_text(encoding='utf-8'))
for request_name in ['batchPreviewRequest', 'batchCreateRequest', 'batchGetRequest']:
    expected_ref = f"{g04f1_schema['$id']}#/$defs/{request_name}"
    if g04f1_ipc_schema.get('$defs', {}).get(request_name) != {'$ref': expected_ref}:
        raise SystemExit(f'G04F1 IPC schema ref is missing or not exact: {request_name}')
for required_boundary in [
    'BATCH_PLAN_STALE', 'parse_naming_template', 'windows_file_names_equal',
    'verify_available_read_only', 'require_preview_proof', 'TransactionBehavior::Immediate',
    'canonical_path_sha256', 'destination_entry_names', 'plan_collisions_against_names',
]:
    if required_boundary not in g04f1_batch and required_boundary not in g04f1_database:
        raise SystemExit(f'G04F1 backend boundary is missing: {required_boundary}')
for forbidden_boundary in ['request.requested_names', '.to_lowercase()', 'BATCH_PREVIEW_STALE']:
    if forbidden_boundary in g04f1_batch:
        raise SystemExit(f'G04F1 retained obsolete batch behavior: {forbidden_boundary}')
g04f1_production_batch = g04f1_batch.split('#[cfg(test)]', 1)[0]
if '.get_or_prepare()' in g04f1_production_batch:
    raise SystemExit('G04F1 preview may not materialize the qpdf runtime cache')
if 'FILE_SHARE_DELETE' in g04f1_production_batch:
    raise SystemExit('G04F1 destination guard may not allow delete sharing')
preview_response = json.dumps(g04f1_schema['$defs']['batchPreviewResponse'], sort_keys=True)
for forbidden_field in ['sourcePath', 'canonicalPath', 'destinationDirectory', 'fileIdentity', 'modifiedAt']:
    if forbidden_field in preview_response:
        raise SystemExit(f'G04F1 preview response leaks a private field: {forbidden_field}')
for required_sql in [
    'CREATE TABLE batch_runs', 'CREATE TABLE batch_run_jobs',
    'batch_runs_one_live_plan_idx', "WHERE state IN ('queued', 'active')",
]:
    if required_sql not in g04f1_migration:
        raise SystemExit(f'G04F1 migration boundary is missing: {required_sql}')
if desktop_scripts.get('verify:g04f1') != 'powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/verify-g04f1-boundaries.ps1':
    raise SystemExit('G04F1 boundary verifier is not registered exactly')
if 'npm run verify:g04f1 --workspace @document-studio/desktop' not in ci_text:
    raise SystemExit('G04F1 boundary verifier is not wired into exact-head CI')
if 'Rust is authoritative for the Windows limit of 255 UTF-16 code units.' not in json.dumps(g04f1_schema):
    raise SystemExit('G04F1 schema must document the authoritative Rust UTF-16 filename gate')

g04e1_core = (ROOT / 'apps/desktop/src-tauri/src/text_to_pdf.rs').read_text(encoding='utf-8')
g04e1_renderer = (ROOT / 'apps/desktop/src-tauri/src/text_to_pdf_renderer.rs').read_text(encoding='utf-8')
g04e1_service = (ROOT / 'apps/desktop/src-tauri/src/text_to_pdf_service.rs').read_text(encoding='utf-8')
g04e1_registry = (ROOT / 'apps/desktop/src-tauri/src/operation_registry.rs').read_text(encoding='utf-8')
g04e1_font_manifest = json.loads(
    (ROOT / 'apps/desktop/src-tauri/resources/fonts/g04e1/font-manifest.json').read_text(encoding='utf-8')
)
g04e1_ipc_schema = json.loads((ROOT / 'packages/contracts/ipc.schema.json').read_text(encoding='utf-8'))
g04e1_request = g04e1_ipc_schema.get('$defs', {}).get('textToPdfJobCreateRequest', {})
if g04e1_request.get('additionalProperties') is not False:
    raise SystemExit('G04E1 TXT-to-PDF IPC request must be closed')
if g04e1_request.get('properties', {}).get('operationId') != {'const': 'text.to-pdf'}:
    raise SystemExit('G04E1 TXT-to-PDF IPC request must be operation-exact')
for required_boundary in [
    'TXT_MAX_RAW_BYTES: usize = 8_388_608',
    'TXT_MAX_LOGICAL_LINES: usize = 100_000',
    'TXT_MAX_LINE_BYTES: usize = 65_536',
    'TXT_INVALID_UTF8',
    'TXT_UNSUPPORTED_BOM',
    'TXT_UNSUPPORTED_UNICODE',
    'TXT_SHAPING_COMPLEXITY_LIMIT',
    "default-src 'none'",
    'font-synthesis:none',
]:
    if required_boundary not in g04e1_core:
        raise SystemExit(f'G04E1 input/document boundary is missing: {required_boundary}')
for required_boundary in [
    'ICoreWebView2_22',
    'ICoreWebView2Environment6',
    'ICoreWebView2_7',
    'AddWebResourceRequestedFilterWithRequestSourceKinds',
    'COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL',
    'COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL',
    'SHCreateMemStream',
    'stream.Stat',
    'WebResourceResponseReceivedEventHandler',
    'DOMContentLoadedEventHandler',
    'PrintToPdf',
    'owns_generation',
]:
    if required_boundary not in g04e1_renderer:
        raise SystemExit(f'G04E1 WebView2 boundary is missing: {required_boundary}')
g04e1_renderer_production = g04e1_renderer.rsplit('#[cfg(test)]', 1)[0]
for forbidden_boundary in [
    'ExecuteScript', 'QueryInterface', 'transmute', 'AddRef',
    'http://', 'localhost', '127.0.0.1', 'file://', 'data:',
]:
    if forbidden_boundary in g04e1_renderer_production:
        raise SystemExit(f'G04E1 renderer contains a prohibited production path: {forbidden_boundary}')
for required_boundary in [
    'publish_verified_staging_with_observer',
    'source.verify_unchanged_hash',
    'TXT_FINAL_HASH_MISMATCH',
    'FontFile2',
    '/OpenAction',
    '/JavaScript',
    '/AcroForm',
    'validate_recovery_renderer_workspaces',
    'id: "webview2"',
]:
    if required_boundary not in g04e1_service:
        raise SystemExit(f'G04E1 verification/recovery boundary is missing: {required_boundary}')
if 'text_to_pdf_manifest()' not in g04e1_registry or 'TEXT_TO_PDF_OPERATION_ID' not in g04e1_registry:
    raise SystemExit('G04E1 operation is not registered exactly')
if g04e1_font_manifest.get('operation') != 'text.to-pdf@1.0.0':
    raise SystemExit('G04E1 font manifest operation is not exact')
fonts = g04e1_font_manifest.get('fonts', [])
if len(fonts) != 3 or {font.get('postScriptName') for font in fonts} != {
    'NotoSans-Regular', 'NotoSansDevanagari-Regular', 'NotoSansTelugu-Regular',
}:
    raise SystemExit('G04E1 font manifest inventory is not exact')
if any(font.get('weight') != 400 or font.get('style') != 'Regular' or font.get('spdxLicense') != 'OFL-1.1' for font in fonts):
    raise SystemExit('G04E1 font manifest admits a non-Regular or non-OFL face')
if desktop_scripts.get('verify:g04e1') != 'powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/verify-g04e1-boundaries.ps1':
    raise SystemExit('G04E1 boundary verifier is not registered exactly')
if 'npm run verify:g04e1 --workspace @document-studio/desktop' not in ci_text:
    raise SystemExit('G04E1 boundary verifier is not wired into exact-head CI')
for native_test in [
    'native_renderer_fault_matrix_closes_exact_generation_at_bounded_checkpoints',
    'native_text_service_cancellation_matrix_cleans_owned_state_and_preserves_commits',
    'native_webview2_qpdf_acceptance_covers_all_page_settings_and_mixed_scripts',
]:
    if native_test not in ci_text:
        raise SystemExit(f'G04E1 native exact-head CI proof is missing: {native_test}')
if any(path.name.startswith('0009_') for path in (ROOT / 'apps/desktop/src-tauri/migrations').glob('*.sql')):
    raise SystemExit('G04E1 added an unauthorized database migration')

g04dc_workflow = (ROOT / '.github/workflows/g04d-c-libreoffice-runtime-proof.yml').read_text(encoding='utf-8')
g04dc_common = (ROOT / 'scripts/g04d-c/G04DC.Common.psm1').read_text(encoding='utf-8')
g04dc_registry_digest = (ROOT / 'scripts/g04d-c/ClassRegistryDigest.cs').read_text(encoding='utf-8')
g04dc_sandbox = (ROOT / 'scripts/g04d-c/G04DC.Sandbox.cs').read_text(encoding='utf-8')
g04dc_sandbox_wrapper = (ROOT / 'scripts/g04d-c/Invoke-G04DCSandboxSmoke.ps1').read_text(encoding='utf-8')
g04dc_admin = (ROOT / 'scripts/g04d-c/Invoke-G04DCAdminImageProof.ps1').read_text(encoding='utf-8')
g04dc_precheck = (ROOT / 'scripts/g04d-c/Invoke-G04DCMachineStatePrecheck.ps1').read_text(encoding='utf-8')
g04dc_minimal = (ROOT / 'scripts/g04d-c/Invoke-G04DCMinimalMsiProof.ps1').read_text(encoding='utf-8')
g04dc_manifest = (ROOT / 'scripts/g04d-c/New-G04DCRuntimeManifest.ps1').read_text(encoding='utf-8')
g04dc_decision = (ROOT / 'scripts/g04d-c/New-G04DCCandidateDecision.ps1').read_text(encoding='utf-8')
g04dc_tests = (ROOT / 'scripts/g04d-c/Test-G04DCBoundaries.ps1').read_text(encoding='utf-8')
g04dc_source_gate = (ROOT / 'scripts/g04d-c/Test-G04DCPowerShell51Source.ps1').read_text(encoding='ascii')
g04dc_boundary_verifier = (ROOT / 'scripts/g04d-c/verify-g04d-c-boundaries.ps1').read_text(encoding='ascii')
g04dc_source_policy = (ROOT / 'scripts/g04d_c_powershell_source_policy.py').read_text(encoding='utf-8')
if 'workflow_dispatch:' not in g04dc_workflow or re.search(r'^\s+(push|pull_request|schedule|workflow_run|repository_dispatch):', g04dc_workflow, re.MULTILINE):
    raise SystemExit('G04D-C proof workflow must remain workflow_dispatch only')
for job in ['machine-state-precheck:', 'admin-image:', 'minimal-msi:', 'decision:']:
    if job not in g04dc_workflow:
        raise SystemExit(f'G04D-C fresh-runner workflow job is missing: {job}')
for action in re.findall(r'^\s*-\s+uses:\s*([^\s]+)', g04dc_workflow, re.MULTILINE):
    if not re.search(r'@[0-9a-f]{40}$', action):
        raise SystemExit(f'G04D-C workflow action is not pinned: {action}')
if len(re.findall(r'persist-credentials:\s*false', g04dc_workflow)) != 4:
    raise SystemExit('G04D-C checkout credentials must not persist onto proof runners')
for boundary in [
    'proofMode:', '- precheck', '- full', "inputs.proofMode == 'precheck'", "inputs.proofMode == 'full'",
    'timeout-minutes: 20', 'g04d-c-machine-state-precheck-evidence',
    'Initialize fail-safe PRECHECK bootstrap evidence', 'bootstrap-source-validation.json',
    'Test-G04DCPowerShell51Source.ps1',
]:
    if boundary not in g04dc_workflow:
        raise SystemExit(f'G04D-C PRECHECK workflow boundary is missing: {boundary}')
if len(re.findall(r'^\s+timeout-minutes:\s*90\s*$', g04dc_workflow, re.MULTILINE)) != 2:
    raise SystemExit('G04D-C FULL proof jobs must retain the 90-minute timeout')
if re.search(r'(?i)msiexec|/a|soffice|Invoke-G04DCSandboxSmoke|AppContainer', g04dc_precheck):
    raise SystemExit('G04D-C PRECHECK may not extract, install or launch the runtime')
for boundary in [
    'machine-pre.json', 'machine-state-progress.ndjson', 'machine-state-performance.json',
    'MACHINE_STATE_PRECHECK_PASS', 'MACHINE_STATE_PRECHECK_BLOCKED',
    'Assert-G04DCRunnerIsolation', 'Assert-G04DCMsiRegistrationAbsent',
    'candidateClassificationProduced = $false',
]:
    if boundary not in g04dc_precheck:
        raise SystemExit(f'G04D-C PRECHECK proof boundary is missing: {boundary}')
for boundary in [
    'bootstrap-source-validation.json', 'precheckScriptStarted',
    'PRECHECK_BOOTSTRAP_INVALID', 'checkedOutSha', 'asciiGateStatus', 'parserGateStatus',
]:
    if boundary not in g04dc_precheck + g04dc_workflow:
        raise SystemExit(f'G04D-C PRECHECK bootstrap boundary is missing: {boundary}')
for boundary in [
    '[System.Management.Automation.Language.Parser]::ParseFile',
    "PSEdition -cne 'Desktop'", 'sourceFileCount', 'asciiGateStatus', 'parserGateStatus',
    'incompleteTokenCount', 'malformedStringTokenCount', 'unknownTokenCount',
    'Windows PowerShell version:', 'ASCII-byte gate result: PASS',
    'Windows PowerShell 5.1 parser gate result: PASS',
]:
    if boundary not in g04dc_source_gate:
        raise SystemExit(f'G04D-C Windows PowerShell 5.1 source gate is missing: {boundary}')
for boundary in [
    'path.read_bytes()', 'KNOWN_BOMS', 'ALLOWED_CONTROL_BYTES',
    '0x20 <= value <= 0x7E', 'relative_to(repository_root)',
    'sourceFileCount', 'asciiGateStatus', 'violations',
]:
    if boundary not in g04dc_source_policy:
        raise SystemExit(f'G04D-C raw-byte source policy is missing: {boundary}')
if 'Test-G04DCPowerShell51Source.ps1' not in ci_text or 'Test-G04DCPowerShell51Source.ps1' not in g04dc_boundary_verifier:
    raise SystemExit('G04D-C source compatibility gate is not wired into normal Windows CI and the focused verifier')
for prohibited_source_dependency in ['chcp 65001', '$OutputEncoding', 'Invoke-Expression']:
    if prohibited_source_dependency in g04dc_source_gate:
        raise SystemExit(f'G04D-C source gate relies on a prohibited decoding path: {prohibited_source_dependency}')
for regression in [
    'ASCII-only .ps1 source passes', 'ASCII-only .psm1 source passes',
    'UTF-8-no-BOM em dash source rejected', 'UTF-8-no-BOM smart quote source rejected',
    'UTF-8 BOM source rejected', 'UTF-16 LE BOM source rejected',
    'UTF-16 BE BOM source rejected', 'UTF-32 LE BOM source rejected',
    'UTF-32 BE BOM source rejected', 'non-ASCII source comment rejected',
    'non-ASCII source string rejected', 'invalid ASCII PowerShell source rejected by parser',
    'valid ASCII PowerShell script accepted by Windows PowerShell 5.1',
    'documentation Unicode remains allowed', 'non-executable explicit encoding fixtures remain allowed',
    'repository validation reports exact source path and byte offset',
    'current PRECHECK source passes ASCII and parser gates',
    'every workflow-invoked G04D-C source passes both gates',
]:
    if regression not in g04dc_tests:
        raise SystemExit(f'G04D-C source compatibility regression is missing: {regression}')
for boundary in [
    'New-G04DCMachineStateCaptureContext', 'Write-G04DCMachineStateProgressRecord',
    'Write-G04DCMachineStateSubstageRecord', 'Start-G04DCMachineStateSubstage',
    'Complete-G04DCMachineStateSubstage',
    'Assert-G04DCMachineStateCaptureBudget', 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED',
    'captureTargetMilliseconds', 'hardCeilingMilliseconds', 'phaseCeilingMilliseconds',
    'Get-G04DCMsiComponentRegistrationState', 'RegistryView]::Registry64',
    'canonical-state-finalization', 'state-serialization', 'Write-G04DCBoundedMachineStateJson',
    'Get-G04DCBoundedFileSha256', 'Invoke-G04DCBoundedCaptureProcess',
    'DOCUMENT-STUDIO-G04DC-CANONICAL-HASH-OWNED', 'Canonical hash input exceeded the 128 MiB ceiling',
    'KillOnCloseJob', 'TerminateAndVerify', 'Assert-G04DCMachineStatePerformanceEvidence',
]:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C bounded machine-state boundary is missing: {boundary}')
for boundary in [
    'ClassRegistryDigestCollector', 'BoundedTextCapture', 'BeginRead', 'AppendStderrText',
    'StringComparer.CurrentCultureIgnoreCase', 'SHA256.Create()', 'REGISTRY_DIGEST_TIMEOUT',
    'REGISTRY_DIGEST_RAW_BYTE_CEILING', 'REGISTRY_DIGEST_ROW_CEILING',
    'REGISTRY_DIGEST_CANONICAL_BYTE_CEILING', 'REGISTRY_DIGEST_STDERR_CEILING',
]:
    if boundary not in g04dc_registry_digest:
        raise SystemExit(f'G04D-C class-registry streaming boundary is missing: {boundary}')
for boundary in [
    'DirectClassRegistryDigestCollector', 'RegistryHive.ClassesRoot', 'RegistryView.Registry64',
    'RegQueryValueExW', 'CollectClassesRoot64', 'REGISTRY_TRAVERSAL_ACCESS_DENIED',
    'REGISTRY_TRAVERSAL_UNSTABLE', 'REGISTRY_TRAVERSAL_KEY_CEILING',
    'REGISTRY_TRAVERSAL_VALUE_CEILING', 'REGISTRY_TRAVERSAL_DEPTH_CEILING',
    'REGISTRY_TRAVERSAL_VALUE_BYTE_CEILING', 'REGISTRY_TRAVERSAL_CANONICAL_BYTE_CEILING',
    'public int SchemaVersion { get { return 2; } }',
]:
    if boundary not in g04dc_registry_digest:
        raise SystemExit(f'G04D-C direct HKCR boundary is missing: {boundary}')
if re.search(r'(?i)CreateSubKey|SetValue|Delete(SubKey|Value)|WriteAll(Text|Bytes)|FileMode\.Create|RegistryKey\.OpenRemoteBaseKey', g04dc_registry_digest):
    raise SystemExit('G04D-C class-registry helper may not mutate registry, use remote registry, or create raw-output artifacts')
for boundary in [
    'native-query-startup', 'native-query-read', 'row-normalization', 'canonical-hash', 'helper-cleanup',
    'MaximumRawBytes = 134217728', 'MaximumRows = 1000000', 'StandardOutputEncoding',
    'ActiveHandleCount', 'nativeCleanupFailureStage', 'secondaryNativeCleanupFailure',
    'MACHINE_STATE_CAPTURE_EVIDENCE_FAILED', 'LifecycleTestHooks',
    'DisposeStdoutReader', 'DisposeStdoutStream', 'DisposeStderrReader', 'DisposeStderrStream',
    'DisposeStdoutTask', 'DisposeStderrTask',
]:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C class-registry process boundary is missing: {boundary}')
for boundary in ['classRegistryDigestTargetMilliseconds = 180000L', 'CLASS_REGISTRY_DIGEST_TARGET_EXCEEDED']:
    if boundary not in g04dc_precheck:
        raise SystemExit(f'G04D-C PRECHECK class-registry target is missing: {boundary}')
for boundary in [
    '$maximumAttempts = 20', 'Start-Sleep -Milliseconds 500', 'DIRECTORY_TREE_CAPTURE_UNSTABLE',
    '$afterItem.LastWriteTimeUtc', "'IOException', 'ItemNotFoundException'",
]:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C bounded directory retry boundary is missing: {boundary}')
for regression in [
    'current native-row normalization equivalence', 'incremental hash equality',
    '64236-row registry digest scale', 'explicit UTF-8 native output encoding',
    'split multibyte character across stream buffers', 'truncated multibyte native output rejected',
    'class registry substage ordering', 'class registry helper termination',
    'registry key creation mutation equivalence', 'registry default absent-empty distinction',
    'registry value-kind mutation equivalence', 'registry Unicode mutation equivalence',
    'optimized class registry collector performs no mutation', 'C7 class registry target is 180000 ms',
    'direct HKCR Registry64 merged-view fixture', 'direct HKCR access denial fails closed',
    'direct HKCR disappearing key fails closed', 'direct HKCR disappearing value fails closed',
    'class registry digest schema change', 'class registry key-count change', 'class registry value-count change',
    'direct HKCR traversal timeout', 'direct HKCR handles disposed for owned cleanup',
    'shortcut catalog transient sharing retry preserves full entry',
    'shortcut catalog persistent sharing failure remains fail closed',
    'P1-A cleanup error precedence preserves secondary evidence',
    'P1-A redirected readers streams and tasks are disposed',
    'P1-A production cleanup uses no global process termination',
    'P1-B canonical hash independent of Console OutputEncoding',
    'P1-B accepted canonical fixture remains byte-identical', 'Expected 315 fail-closed cases',
]:
    if regression not in g04dc_tests:
        raise SystemExit(f'G04D-C class-registry regression is missing: {regression}')
for boundary in [
    'CanonicalUtf8 = new UTF8Encoding(false, true)', 'nativeOutputEncoding',
    'CanonicalUtf8.GetByteCount', 'CanonicalUtf8.GetBytes',
]:
    if boundary not in g04dc_registry_digest:
        raise SystemExit(f'G04D-C canonical UTF-8 boundary is missing: {boundary}')
for identity in [
    '372948992', 'f15ba07bfcb0186986cf3171063506f5d207c11f8cc051ba0d135209e9e915f9',
    '{3B467719-C25B-478C-8F4C-8E2EDA0E2093}', '{4B17E523-5D91-4E69-BD96-7FD81CFA81BB}',
    '{5D7F0329-EE50-4638-9909-70F6CEB181D0}', '6480532A562B36D1BFFFC5B5EACF7C31E74E9B28',
    '571468410CA85AF3424EF9164A513610F4D38D98',
]:
    if identity not in g04dc_common:
        raise SystemExit(f'G04D-C exact MSI identity is missing: {identity}')
for boundary in [
    'CreateAppContainerProfile', 'CREATE_SUSPENDED', 'AssignProcessToJobObject',
    'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE', 'JOB_OBJECT_LIMIT_ACTIVE_PROCESS',
    'JOB_OBJECT_LIMIT_JOB_MEMORY', 'DocumentStudio.OfficeEngine.LibreOffice.G04DC.Proof',
]:
    if boundary not in g04dc_sandbox + g04dc_sandbox_wrapper:
        raise SystemExit(f'G04D-C sandbox boundary is missing: {boundary}')
if (ROOT / 'scripts/g04d-c/Invoke-G04DCDirectSmoke.ps1').exists() or 'Invoke-G04DCDirectSmoke' in g04dc_admin + g04dc_minimal:
    raise SystemExit('G04D-C must not retain an unsandboxed LibreOffice runtime path')
for boundary in ['--version', '--convert-to', 'Invoke-G04DCZeroCapabilityProbe']:
    if boundary not in g04dc_sandbox_wrapper:
        raise SystemExit(f'G04D-C sandbox-only runtime probe is missing: {boundary}')
for boundary in [
    'allRegistryRows', 'serviceCatalogSha256', 'scheduledTaskCatalogSha256',
    'firewallCatalogSha256', 'installedProductCatalogSha256', 'otherInstalledProductCatalogSha256', 'installerCacheCatalogSha256', 'serviceRegistryCatalogSha256',
    'classRegistryCatalogSchemaVersion', 'classRegistryKeyCount', 'classRegistryValueCount',
    'classRegistryCatalogSha256', 'shortcutCatalogSha256', 'environmentCatalogSha256', 'pendingReboot',
]:
    if boundary not in g04dc_common + g04dc_admin + g04dc_minimal:
        raise SystemExit(f'G04D-C full non-mutation seal is missing: {boundary}')
for boundary in ['RunOnce', r'WOW6432Node\Microsoft\Windows\CurrentVersion\Run', 'SpecialFolder]::Startup', 'ProgramData']:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C startup catalog seal is missing: {boundary}')
for boundary in ['$process.ExitCode -eq 0', '$uninstall.ExitCode -eq 0', 'exactProductRegistration', 'REBOOT_REQUIRED', 'post-uninstall-install-root-residue.json']:
    if boundary not in g04dc_minimal:
        raise SystemExit(f'G04D-C minimal-MSI lifecycle gate is missing: {boundary}')
for boundary in ['INSTALLLEVEL=0', 'REMOVE=$removeFeatureList', 'MsiConditionEvaluator', 'Resolve-G04DCExpectedComponentStates', 'Get-G04DCMutationClosure', 'Assert-G04DCMinimalMutationClosure', 'Get-G04DCInstalledFeatureStates', 'Assert-G04DCInstalledFeatureStates', 'Assert-G04DCInstalledComponentStates', 'Assert-G04DCInstalledFileOwnership', 'Assert-G04DCMsiRegistrationInstalled']:
    if boundary not in g04dc_minimal:
        raise SystemExit(f'G04D-C minimal installed feature/file gate is missing: {boundary}')
for boundary in ['TotalAssignedProcesses', 'totalAssignedProcesses', 'SandboxRunException']:
    if boundary not in g04dc_sandbox + g04dc_common:
        raise SystemExit(f'G04D-C complete process evidence is missing: {boundary}')
for boundary in ['loadedModules', 'moduleInventoryComplete', 'Get-G04DCLoadBearingModuleEvidence', 'sandbox-load-bearing-modules.json', 'O=Microsoft Corporation']:
    if boundary not in g04dc_sandbox + g04dc_sandbox_wrapper + g04dc_common:
        raise SystemExit(f'G04D-C dynamic module provenance boundary is missing: {boundary}')
if 'potentialExecutable' not in g04dc_manifest or 'staticallyAdmittedEntryPoint' not in g04dc_manifest or 'invalidLoadBearing' in g04dc_manifest:
    raise SystemExit('G04D-C runtime manifest blurs static PE inventory with dynamically loaded modules')
for boundary in ['signerChainElements', 'timestampChainElements', '@($Identity.signerChain).Count -ge 2']:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C MSI chain evidence is missing: {boundary}')
for boundary in ["'LoopbackExempt' '-s'", 'TCP and UDP owned sockets/listeners sampled']:
    if boundary not in g04dc_sandbox_wrapper:
        raise SystemExit(f'G04D-C network boundary is missing: {boundary}')
for boundary in ['ownedWritablePathInventory', 'appContainerExternalStorageAbsent', 'appContainerRegistryResidueAbsent', 'runtimeTreeUnchanged', 'GetAppContainerFolderPath', 'actualAccessTelemetryCaptured', 'effectiveDenialOutsideAllowedRootsProven']:
    if boundary not in g04dc_sandbox_wrapper + g04dc_common:
        raise SystemExit(f'G04D-C file-access boundary is missing: {boundary}')
if 'actualAccessTelemetryCaptured = $false' not in g04dc_sandbox_wrapper or 'effectiveDenialOutsideAllowedRootsProven = $false' not in g04dc_sandbox_wrapper or 'Assert-G04DCFileAccessEvidence' not in g04dc_sandbox_wrapper:
    raise SystemExit('G04D-C must reject admission when empirical effective file-access telemetry is unavailable')
for boundary in ['ProductState', 'LocalPackage', r'Installer\Products', r'Installer\Features', r'Installer\UpgradeCodes', r'Installer\UserData\S-1-5-18\Components', 'Assert-G04DCMsiRegistrationAbsent']:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C authoritative Windows Installer boundary is missing: {boundary}')
for boundary in ['RemoveRegistry', 'Extension', 'ProgId', 'MIME', 'Verb', 'Class', 'AppId', 'Shortcut', 'Environment', 'AdminExecuteSequence', 'AdminUISequence', 'unboundedInstallCustomActions', 'unboundedAdminCustomActions']:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C MSI effect/action model is missing: {boundary}')
for boundary in ['markerOwnedPathsOnly', 'reparseEntryCount', 'markerSha256', 'Remove-G04DCOwnedRoot']:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C marker-owned cleanup boundary is missing: {boundary}')
if 'Remove-G04DCOwnedRoot' not in g04dc_admin or 'Remove-G04DCOwnedRoot' not in g04dc_minimal:
    raise SystemExit('G04D-C modes do not share exact owned-root cleanup on every terminal path')
post_smoke = '$smokeComparison = Compare-G04DCMachineState -Before $before -After $afterSmoke'
sandbox_pass = '$sandboxPassed = $true'
if post_smoke not in g04dc_minimal or sandbox_pass not in g04dc_minimal or g04dc_minimal.index(post_smoke) > g04dc_minimal.index(sandbox_pass):
    raise SystemExit('G04D-C minimal proof sets sandboxPassed before post-smoke machine reconciliation')
if 'post-uninstall-install-root-residue.json' not in g04dc_minimal or '-and ![bool]$installRootResidue.present' not in g04dc_minimal:
    raise SystemExit('G04D-C minimal proof does not reject install-root residue before cleanup')
for boundary in [
    'TcpClient]::new', '$tcp.Connect($selected, 443)', 'AuthenticateAsClient',
    'RemoteEndPoint', 'Assert-G04DCPinnedRemoteEndpoint', 'CanonicalFirstRequest', 'maximumRedirects = 8',
    'Redirect loop detected', 'Raw IP-literal acquisition hosts are prohibited',
    'Test-G04DCRestrictedIpAddress', 'FileMode]::CreateNew', 'FileShare]::None',
    'Assert-G04DCBoundedDownloadLength', 'Assert-G04DCFailedDownloadCleanup',
    'mirrorHostnameIsTrustAnchor = $false', 'MSI_ACQUISITION_SOURCE_REJECTED',
]:
    if boundary not in g04dc_common:
        raise SystemExit(f'G04D-C MirrorBrain acquisition gate is missing: {boundary}')
if 'Invoke-WebRequest' in g04dc_common or 'HttpWebRequest' in g04dc_common:
    raise SystemExit('G04D-C acquisition may not delegate DNS or redirect handling to an unpinned HTTP client')
for regression in [
    'exact canonical first request', 'accepted HTTPS cross-origin MirrorBrain redirect',
    'pinned HTTPS remote endpoint', 'DNS rebinding remote endpoint', 'multi-hop HTTPS redirect',
    'exact eight-hop redirect boundary', 'ninth-hop rejection',
    'redirect loop', 'redirect missing Location', 'HTTP downgrade', 'non-default port',
    'acquisition URI userinfo', 'acquisition URI empty userinfo', 'acquisition URI fragment', 'localhost target',
    'IPv4 loopback target', 'IPv6 loopback target', 'RFC1918 private target',
    'link-local target', 'multicast target', 'reserved target', 'unspecified target',
    'raw IP literal target', 'unexpected final filename or path', 'truncated acquisition body',
    'oversized acquisition body', 'bounded chunked acquisition body', 'failed-download cleanup ownership',
    'complete redirect-chain evidence', 'no production acquisition path',
]:
    if regression not in g04dc_tests:
        raise SystemExit(f'G04D-C MirrorBrain failure regression is missing: {regression}')
if 'if: ${{ always() }}' not in g04dc_workflow or 'PROOF_PROVENANCE_OR_INFRASTRUCTURE_FAILURE' not in g04dc_workflow or 'Assert-G04DCArtifactManifest' not in g04dc_decision:
    raise SystemExit('G04D-C terminal decision does not separate evidence rejection from infrastructure/provenance failure')
for regression in [
    'MSI condition evaluation error', 'loaded module root or signature rejected', 'artifact manifest hash mismatch',
    'ambiguous MSI effect ownership', 'enabled MSI shortcut mutation', 'unbounded install custom action',
    'unbounded administrative custom action', 'Windows Installer registration residue', 'Windows Installer cache residue',
    'service registry configuration change', 'full classes registry change',
    'desktop or start-menu shortcut change', 'environment registry change', 'file-access observation incomplete',
]:
    if regression not in g04dc_tests:
        raise SystemExit(f'G04D-C failure regression is missing: {regression}')

legacy_name = 'Rohith' + ' Document Studio'
for p in ROOT.rglob('*.md'):
    if 'attachments/archive' in str(p):
        continue
    if legacy_name in p.read_text(encoding='utf-8', errors='ignore'):
        raise SystemExit(f'Legacy name found outside archive: {p.relative_to(ROOT)}')

print(
    'Repository validation passed. '
    f'{len(rows)} feature entries and {g04dc_source_report["sourceFileCount"]} ASCII-only '
    'G04D-C PowerShell sources found; G01-G04F1, G04D-C and G04E1 TXT-to-PDF boundaries verified.'
)
