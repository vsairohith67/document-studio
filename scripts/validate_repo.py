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
        'g04b — active implementation',
        'g04b is not accepted or complete',
        'pdf-to-images',
        'dependency-blocked',
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
    for required in ['to_ascii_lowercase', 'contains("--remote-debugging-")', 'contains("--remote-allow-origins")']:
        if required not in runtime:
            raise SystemExit(f'G03 production remote-debug family sanitizer is missing {required}')
    if 'renderForms: false' not in str(inputs['session']) or 'AnnotationMode.DISABLE' not in str(inputs['session']):
        raise SystemExit('G03 form/annotation rendering boundary is not explicit')


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

    convert = inputs['convert']
    for required in [
        'PDF to images · dependency blocked',
        'No accepted production renderer passed the provenance and license gate',
        'event.altKey',
        "event.key === 'Delete'",
        'api.jobs.warnings',
    ]:
        if required not in convert:
            raise SystemExit(f'G04B truthful accessible UI is missing: {required}')

    rust_production = inputs['rust_production']
    if 'pdf.to-images' in rust_production:
        raise SystemExit('G04B contains an unauthorized production PDF-to-images renderer path')


G04B_VALIDATION_INPUTS = {
    'cargo': (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text(encoding='utf-8'),
    'contracts': (ROOT / 'apps/desktop/src-tauri/src/contracts.rs').read_text(encoding='utf-8'),
    'writer': (ROOT / 'apps/desktop/src-tauri/src/image_to_pdf.rs').read_text(encoding='utf-8'),
    'image_tests': (ROOT / 'apps/desktop/src-tauri/tests/image_to_pdf.rs').read_text(encoding='utf-8'),
    'viewer_tests': (ROOT / 'apps/desktop/e2e/viewer.spec.ts').read_text(encoding='utf-8'),
    'convert': (ROOT / 'apps/desktop/src/ConvertWorkspace.tsx').read_text(encoding='utf-8'),
    'rust_production': '\n'.join(
        path.read_text(encoding='utf-8')
        for path in (ROOT / 'apps/desktop/src-tauri/src').rglob('*.rs')
    ),
}
validate_g04b_boundaries(G04B_VALIDATION_INPUTS)

legacy_name = 'Rohith' + ' Document Studio'
for p in ROOT.rglob('*.md'):
    if 'attachments/archive' in str(p):
        continue
    if legacy_name in p.read_text(encoding='utf-8', errors='ignore'):
        raise SystemExit(f'Legacy name found outside archive: {p.relative_to(ROOT)}')

print(
    'Repository validation passed. '
    f'{len(rows)} feature entries found; G01-G04A accepted status and G04B dependency, migration, scope and document consistency verified.'
)
