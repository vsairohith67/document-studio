import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

const repoRoot = resolve(import.meta.dirname, '..', '..', '..');
const cargoTargetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(repoRoot, process.env.CARGO_TARGET_DIR)
  : resolve(repoRoot, 'target');
const evidenceRoot = process.env.DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT
  ? resolve(process.env.DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT)
  : cargoTargetRoot;
const evidenceDirectory = resolve(evidenceRoot, 'g04e1-browser-visual-evidence');
const evidencePath = resolve(evidenceDirectory, 'mixed-a4-portrait.pdf');

mkdirSync(evidenceDirectory, { recursive: true });
rmSync(evidencePath, { force: true });

const result = spawnSync(
  'cargo',
  [
    'test', '-p', 'document-studio', '--lib',
    'text_to_pdf_service::tests::native_webview2_qpdf_acceptance_covers_all_page_settings_and_mixed_scripts',
    '--locked', '--', '--ignored', '--exact', '--nocapture',
  ],
  {
    cwd: repoRoot,
    env: {
      ...process.env,
      DOCUMENT_STUDIO_G04E1_VISUAL_EVIDENCE_DIR: evidenceDirectory,
    },
    stdio: 'inherit',
  },
);

if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
if (!existsSync(evidencePath)) {
  throw new Error('G04E1 native visual evidence is missing mixed-a4-portrait.pdf.');
}
