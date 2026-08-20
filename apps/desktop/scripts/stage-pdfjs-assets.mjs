import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const desktopRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = join(desktopRoot, '..', '..');
const packageRoot = join(repositoryRoot, 'node_modules', 'pdfjs-dist');
const outputRoot = join(desktopRoot, 'public', 'pdfjs');
const packageMetadata = JSON.parse(await readFile(join(packageRoot, 'package.json'), 'utf8'));
const expectedVersion = '6.2.108';

if (packageMetadata.version !== expectedVersion) {
  throw new Error(`Expected pdfjs-dist ${expectedVersion}, found ${packageMetadata.version}`);
}

const workerSource = join(packageRoot, 'legacy', 'build', 'pdf.worker.mjs');
const workerBytes = await readFile(workerSource);
const workerText = workerBytes.toString('utf8');
const workerVersion = workerText.match(/pdfjsVersion = ([0-9]+\.[0-9]+\.[0-9]+)/u)?.[1];
if (workerVersion !== expectedVersion) {
  throw new Error(`PDF.js worker/API mismatch: API ${expectedVersion}, worker ${workerVersion ?? 'unknown'}`);
}

await mkdir(outputRoot, { recursive: true });
await cp(workerSource, join(outputRoot, 'pdf.worker.mjs'), { force: true });
for (const assetDirectory of ['cmaps', 'standard_fonts', 'iccs', 'wasm']) {
  await cp(join(packageRoot, assetDirectory), join(outputRoot, assetDirectory), {
    force: true,
    recursive: true,
  });
}

const manifest = {
  package: 'pdfjs-dist',
  version: expectedVersion,
  build: 'legacy',
  worker: 'pdf.worker.mjs',
  workerSha256: createHash('sha256').update(workerBytes).digest('hex'),
  localOnly: true,
  assets: ['cmaps', 'standard_fonts', 'iccs', 'wasm'],
};
await writeFile(join(outputRoot, 'pdfjs-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
console.log(`Staged pdfjs-dist ${expectedVersion} local assets (${manifest.workerSha256}).`);
