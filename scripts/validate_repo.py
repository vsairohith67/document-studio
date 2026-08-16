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
    'docs/adr/ADR-005-storage-ownership-and-provider-persistence.md',
    'docs/adr/ADR-006-foundation-dependencies-and-sqlite.md',
    'docs/adr/ADR-007-durable-publication-and-recovery.md',
    'docs/implementation-log/G01-foundation.md',
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
for prohibited in ['pdfjs-dist', 'qpdf', 'libvips', 'ocrmypdf', 'tesseract', 'libreoffice']:
    if re.search(rf'(^|["\s]){re.escape(prohibited)}(["\s=:]|$)', manifest_text, re.IGNORECASE | re.MULTILINE):
        raise SystemExit(f'Out-of-scope G01 dependency found: {prohibited}')

for migration in (ROOT / 'apps/desktop/src-tauri/migrations').glob('*.sql'):
    if re.search(r'\bBLOB\b', migration.read_text(encoding='utf-8'), re.IGNORECASE):
        raise SystemExit(f'Metadata-only migration contains a BLOB column: {migration.relative_to(ROOT)}')

legacy_name = 'Rohith' + ' Document Studio'
for p in ROOT.rglob('*.md'):
    if 'attachments/archive' in str(p):
        continue
    if legacy_name in p.read_text(encoding='utf-8', errors='ignore'):
        raise SystemExit(f'Legacy name found outside archive: {p.relative_to(ROOT)}')

print(
    'Repository validation passed. '
    f'{len(rows)} feature entries found; G01 locks, workspaces, migrations, contracts and privacy rules verified.'
)
