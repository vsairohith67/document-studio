import {
  copyFile,
  lstat,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const EXPECTED_PDFJS_VERSION = '6.2.108';
export const STAGED_MANIFEST_NAME = 'pdfjs-manifest.json';
const scriptPath = fileURLToPath(import.meta.url);
const desktopRoot = join(dirname(scriptPath), '..');
const repositoryRoot = join(desktopRoot, '..', '..');
const defaultPackageRoot = join(repositoryRoot, 'node_modules', 'pdfjs-dist');
const defaultOutputRoot = join(desktopRoot, 'public', 'pdfjs');
const defaultManifestPath = join(dirname(scriptPath), `pdfjs-assets-${EXPECTED_PDFJS_VERSION}.json`);
const approvedWasm = ['jbig2.wasm', 'openjpeg.wasm', 'qcms_bg.wasm'];

function safeRelativePath(value) {
  return typeof value === 'string' && value.length > 0
    && !value.includes('\\') && !value.startsWith('/')
    && value.split('/').every((part) => part && part !== '.' && part !== '..');
}

async function sha256File(path) {
  return createHash('sha256').update(await readFile(path)).digest('hex');
}

async function listFiles(root) {
  const files = [];
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await visit(path);
      else if (entry.isFile()) files.push(relative(root, path).split(sep).join('/'));
      else throw new Error(`PDF.js staging rejects non-regular entry ${entry.name}`);
    }
  }
  await visit(root);
  return files.sort();
}

export function validatePdfjsManifest(manifest) {
  if (manifest?.schemaVersion !== 1 || manifest?.package !== 'pdfjs-dist'
    || manifest?.version !== EXPECTED_PDFJS_VERSION || manifest?.build !== 'legacy'
    || manifest?.worker !== 'pdf.worker.mjs' || manifest?.localOnly !== true
    || !Array.isArray(manifest.files) || manifest.files.length === 0) {
    throw new Error('PDF.js asset manifest metadata is invalid');
  }
  const seen = new Set();
  for (const file of manifest.files) {
    if (!safeRelativePath(file.path) || !safeRelativePath(file.sourcePath)
      || !Number.isSafeInteger(file.sizeBytes) || file.sizeBytes < 1
      || !/^[a-f0-9]{64}$/u.test(file.sha256)
      || !['worker', 'cmap', 'standard-font', 'icc', 'wasm'].includes(file.category)
      || seen.has(file.path)) throw new Error('PDF.js asset manifest entry is invalid');
    const lower = `${file.path} ${file.sourcePath}`.toLocaleLowerCase();
    if (lower.endsWith('.map') || lower.includes('debug') || lower.includes('viewer.html')
      || lower.includes('quickjs') || lower.includes('nowasm_fallback')
      || lower.includes('/web/') || lower.includes('/test/') || lower.includes('/examples/')) {
      throw new Error(`PDF.js asset manifest contains forbidden asset ${file.path}`);
    }
    seen.add(file.path);
  }
  if (!seen.has('pdf.worker.mjs')) throw new Error('PDF.js worker is missing from the manifest');
  return manifest;
}

export async function readPdfjsManifest(path = defaultManifestPath) {
  return validatePdfjsManifest(JSON.parse(await readFile(path, 'utf8')));
}

export async function verifyStagedDirectory(outputRoot, manifest) {
  const expected = [...manifest.files.map((file) => file.path), STAGED_MANIFEST_NAME].sort();
  const actual = await listFiles(outputRoot);
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error('PDF.js staged membership does not match the exact manifest');
  }
  for (const file of manifest.files) {
    const path = join(outputRoot, ...file.path.split('/'));
    const status = await lstat(path);
    if (!status.isFile() || status.isSymbolicLink() || status.size !== file.sizeBytes
      || await sha256File(path) !== file.sha256) {
      throw new Error(`PDF.js staged asset failed verification: ${file.path}`);
    }
  }
  const stagedManifest = JSON.parse(await readFile(join(outputRoot, STAGED_MANIFEST_NAME), 'utf8'));
  if (JSON.stringify(stagedManifest) !== JSON.stringify(manifest)) {
    throw new Error('PDF.js staged manifest differs from the checked-in manifest');
  }
}

export async function stagePdfjsAssets({
  packageRoot = defaultPackageRoot,
  outputRoot = defaultOutputRoot,
  manifestPath = defaultManifestPath,
  faultAfterBackup = false,
} = {}) {
  const manifest = await readPdfjsManifest(manifestPath);
  const packageMetadata = JSON.parse(await readFile(join(packageRoot, 'package.json'), 'utf8'));
  if (packageMetadata.version !== EXPECTED_PDFJS_VERSION) {
    throw new Error(`Expected pdfjs-dist ${EXPECTED_PDFJS_VERSION}, found ${packageMetadata.version}`);
  }
  const worker = manifest.files.find((file) => file.category === 'worker');
  const workerText = await readFile(join(packageRoot, ...worker.sourcePath.split('/')), 'utf8');
  const workerVersion = workerText.match(/pdfjsVersion = ([0-9]+\.[0-9]+\.[0-9]+)/u)?.[1];
  if (workerVersion !== EXPECTED_PDFJS_VERSION) {
    throw new Error(`PDF.js worker/API mismatch: API ${EXPECTED_PDFJS_VERSION}, worker ${workerVersion ?? 'unknown'}`);
  }

  const parent = dirname(outputRoot);
  const name = basename(outputRoot);
  const ownership = randomUUID();
  const temporary = join(parent, `.${name}.tmp-${ownership}`);
  const backup = join(parent, `.${name}.backup-${ownership}`);
  await mkdir(parent, { recursive: true });
  await mkdir(temporary);
  let backupCreated = false;
  try {
    for (const file of manifest.files) {
      const source = join(packageRoot, ...file.sourcePath.split('/'));
      const sourceStatus = await lstat(source);
      if (!sourceStatus.isFile() || sourceStatus.isSymbolicLink()
        || sourceStatus.size !== file.sizeBytes || await sha256File(source) !== file.sha256) {
        throw new Error(`PDF.js package asset failed verification: ${file.sourcePath}`);
      }
      const destination = join(temporary, ...file.path.split('/'));
      await mkdir(dirname(destination), { recursive: true });
      await copyFile(source, destination);
    }
    await writeFile(join(temporary, STAGED_MANIFEST_NAME), `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
    await verifyStagedDirectory(temporary, manifest);
    try {
      await rename(outputRoot, backup);
      backupCreated = true;
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
    if (faultAfterBackup) throw new Error('Injected PDF.js replacement failure');
    try {
      await rename(temporary, outputRoot);
    } catch (error) {
      if (backupCreated) await rename(backup, outputRoot);
      backupCreated = false;
      throw error;
    }
    if (backupCreated) {
      await rm(backup, { recursive: true, force: true });
      backupCreated = false;
    }
    await verifyStagedDirectory(outputRoot, manifest);
    return manifest;
  } finally {
    await rm(temporary, { recursive: true, force: true });
    if (backupCreated) {
      try { await rename(backup, outputRoot); } catch { /* preserve evidence for the caller */ }
    }
  }
}

async function buildManifest() {
  const entries = [{ path: 'pdf.worker.mjs', sourcePath: 'legacy/build/pdf.worker.mjs', category: 'worker' }];
  for (const [directory, category] of [
    ['cmaps', 'cmap'], ['standard_fonts', 'standard-font'], ['iccs', 'icc'],
  ]) {
    for (const name of await listFiles(join(defaultPackageRoot, directory))) {
      entries.push({ path: `${directory}/${name}`, sourcePath: `${directory}/${name}`, category });
    }
  }
  for (const name of approvedWasm) {
    entries.push({ path: `wasm/${name}`, sourcePath: `wasm/${name}`, category: 'wasm' });
  }
  const files = [];
  for (const entry of entries) {
    const source = join(defaultPackageRoot, ...entry.sourcePath.split('/'));
    const status = await lstat(source);
    files.push({ ...entry, sizeBytes: status.size, sha256: await sha256File(source) });
  }
  return validatePdfjsManifest({
    schemaVersion: 1,
    package: 'pdfjs-dist',
    version: EXPECTED_PDFJS_VERSION,
    build: 'legacy',
    worker: 'pdf.worker.mjs',
    localOnly: true,
    files,
  });
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  if (process.argv.includes('--refresh-manifest')) {
    const manifest = await buildManifest();
    await writeFile(defaultManifestPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
    console.log(`Wrote exact PDF.js ${EXPECTED_PDFJS_VERSION} manifest with ${manifest.files.length} assets.`);
  } else {
    const manifest = await stagePdfjsAssets();
    console.log(`Staged and verified pdfjs-dist ${EXPECTED_PDFJS_VERSION} (${manifest.files.length} exact assets).`);
  }
}
