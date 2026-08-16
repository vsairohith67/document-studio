from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
errors = []
pattern = re.compile(r'\[[^\]]+\]\((?!https?://|mailto:|sandbox:|#)([^)]+)\)')
ignored_directories = {
    '.git',
    '.venv',
    'dist',
    'node_modules',
    'target',
}
for md in ROOT.rglob('*.md'):
    if any(part in ignored_directories for part in md.relative_to(ROOT).parts):
        continue
    text = md.read_text(encoding='utf-8', errors='ignore')
    for target in pattern.findall(text):
        target = target.split('#', 1)[0]
        if not target:
            continue
        p = (md.parent / target).resolve()
        if not p.exists():
            errors.append(f'{md.relative_to(ROOT)} -> {target}')
if errors:
    raise SystemExit('Broken local links:\n' + '\n'.join(errors))
print('Internal Markdown link check passed.')
