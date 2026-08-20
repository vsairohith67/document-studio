import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';
import {
  readPdfjsManifest,
  stagePdfjsAssets,
  verifyStagedDirectory,
} from './stage-pdfjs-assets.mjs';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const desktopRoot = join(scriptRoot, '..');
const repositoryRoot = join(desktopRoot, '..', '..');
const packageRoot = join(repositoryRoot, 'node_modules', 'pdfjs-dist');
const manifestPath = join(scriptRoot, 'pdfjs-assets-6.2.108.json');
const root = await mkdtemp(join(tmpdir(), 'document-studio-pdfjs-assets-'));
const output = join(root, 'pdfjs');

async function expectFailure(action, pattern) {
  await assert.rejects(action, pattern);
}

try {
  const manifest = await readPdfjsManifest(manifestPath);
  await stagePdfjsAssets({ packageRoot, outputRoot: output, manifestPath });
  await verifyStagedDirectory(output, manifest);

  await writeFile(join(output, 'stale.txt'), 'stale', 'utf8');
  await stagePdfjsAssets({ packageRoot, outputRoot: output, manifestPath });
  await assert.rejects(readFile(join(output, 'stale.txt')), { code: 'ENOENT' });

  for (const [name, relativePath] of [
    ['unexpected extra asset', 'unexpected.bin'],
    ['source map', 'pdf.worker.mjs.map'],
    ['debug file', 'debug-helper.txt'],
    ['generic viewer UI', 'web/viewer.html'],
  ]) {
    const path = join(output, ...relativePath.split('/'));
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, name, 'utf8');
    await expectFailure(() => verifyStagedDirectory(output, manifest), /membership/u);
    await rm(path, { force: true });
  }

  const first = manifest.files[0];
  await writeFile(join(output, ...first.path.split('/')), 'modified', 'utf8');
  await expectFailure(() => verifyStagedDirectory(output, manifest), /failed verification/u);
  await stagePdfjsAssets({ packageRoot, outputRoot: output, manifestPath });

  const missingManifest = structuredClone(manifest);
  missingManifest.files.push({
    path: 'wasm/missing.wasm', sourcePath: 'wasm/missing.wasm', category: 'wasm',
    sizeBytes: 1, sha256: '0'.repeat(64),
  });
  const missingManifestPath = join(root, 'missing.json');
  await writeFile(missingManifestPath, JSON.stringify(missingManifest), 'utf8');
  await expectFailure(
    () => stagePdfjsAssets({ packageRoot, outputRoot: output, manifestPath: missingManifestPath }),
    /ENOENT|failed verification/u,
  );

  await expectFailure(
    () => stagePdfjsAssets({ packageRoot, outputRoot: output, manifestPath, faultAfterBackup: true }),
    /Injected PDF.js replacement failure/u,
  );
  await verifyStagedDirectory(output, manifest);
  console.log('PDF.js exact-manifest and atomic-staging regressions passed (9 cases).');
} finally {
  await rm(root, { recursive: true, force: true });
}
