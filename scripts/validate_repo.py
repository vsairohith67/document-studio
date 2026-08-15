from pathlib import Path
import csv
import json
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
    'packages/tokens/document-studio.tokens.json',
    'models/models.yaml',
    'apps/prototype/index.html',
    'apps/prototype/screenshots/home.png',
    'apps/prototype/screenshots/workbench.png',
    'apps/desktop/package.json',
    'apps/desktop/src-tauri/Cargo.toml',
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
if not package.get('devDependencies', {}).get('@vitejs/plugin-react', '').startswith('^6.'):
    raise SystemExit('Vite 8 starter must use @vitejs/plugin-react 6.x')

legacy_name = 'Rohith' + ' Document Studio'
for p in ROOT.rglob('*.md'):
    if 'attachments/archive' in str(p):
        continue
    if legacy_name in p.read_text(encoding='utf-8', errors='ignore'):
        raise SystemExit(f'Legacy name found outside archive: {p.relative_to(ROOT)}')

print(f'Repository validation passed. {len(rows)} feature entries found; canonical reports and import packs present.')
