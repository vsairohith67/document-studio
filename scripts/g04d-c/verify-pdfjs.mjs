import { readFile, writeFile } from 'node:fs/promises';
import { getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';

const [pdfPath, evidencePath] = process.argv.slice(2);
if (!pdfPath || !evidencePath) throw new Error('Usage: verify-pdfjs.mjs <pdf> <evidence-json>');

const bytes = new Uint8Array(await readFile(pdfPath));
const task = getDocument({
  data: bytes,
  isEvalSupported: false,
  enableXfa: false,
  useWorkerFetch: false,
  useSystemFonts: false,
  disableFontFace: true,
});
const document = await task.promise;
try {
  if (!Number.isSafeInteger(document.numPages) || document.numPages < 1 || document.numPages > 4096) {
    throw new Error(`PDF.js returned invalid page count ${document.numPages}`);
  }
  const page = await document.getPage(1);
  const viewport = page.getViewport({ scale: 1 });
  if (!(viewport.width > 0) || !(viewport.height > 0)) throw new Error('PDF.js returned invalid page dimensions');
  const text = await page.getTextContent({ disableNormalization: false });
  const joined = text.items.map((item) => ('str' in item ? item.str : '')).join(' ');
  if (!joined.includes('Document Studio G04D-C synthetic runtime smoke')) {
    throw new Error('PDF.js did not recover the synthetic canary text');
  }
  await writeFile(evidencePath, `${JSON.stringify({
    pdfjsOpened: true,
    version: '6.2.108',
    pageCount: document.numPages,
    firstPageWidth: viewport.width,
    firstPageHeight: viewport.height,
    syntheticCanaryPresent: true,
  }, null, 2)}\n`, 'utf8');
  page.cleanup();
} finally {
  await document.destroy();
}
