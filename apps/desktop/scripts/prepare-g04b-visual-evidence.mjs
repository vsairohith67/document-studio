import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

const repoRoot = resolve(import.meta.dirname, '..', '..', '..');
const evidenceDirectory = resolve(repoRoot, 'target', 'g04b-browser-visual-evidence');
const evidenceFiles = ['source.png', 'output.pdf'];

mkdirSync(evidenceDirectory, { recursive: true });
for (const file of evidenceFiles) {
  rmSync(resolve(evidenceDirectory, file), { force: true });
}

const result = spawnSync(
  'cargo',
  [
    'test', '--locked', '--test', 'image_to_pdf',
    'jpeg_png_and_webp_publish_one_verified_page_each_in_selected_order',
    '--', '--exact',
  ],
  {
    cwd: repoRoot,
    env: {
      ...process.env,
      DOCUMENT_STUDIO_G04B_VISUAL_EVIDENCE_DIR: evidenceDirectory,
    },
    stdio: 'inherit',
  },
);

if (result.error) {
  throw result.error;
}
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}
for (const file of evidenceFiles) {
  if (!existsSync(resolve(evidenceDirectory, file))) {
    throw new Error(`G04B native visual evidence is missing ${file}.`);
  }
}
