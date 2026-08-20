import { chromium } from 'playwright';

const port = Number(process.env.DOCUMENT_STUDIO_TEST_CDP_PORT);
if (!Number.isSafeInteger(port) || port < 1024 || port > 65535) {
  throw new Error('A safe test-only WebView2 CDP port is required.');
}
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
try {
  const contexts = browser.contexts();
  const page = contexts.flatMap((context) => context.pages())[0];
  if (!page) throw new Error('The Document Studio WebView2 page was not available.');
  await page.waitForFunction(() => Boolean(globalThis.__TAURI_INTERNALS__?.invoke));
  const result = await page.evaluate(async () => {
    const invoke = globalThis.__TAURI_INTERNALS__.invoke;
    const samples = [];
    const responseTypes = new Set();
    let header = '';
    let byteLength = 0;
    let metadataKeys = [];
    let lastMetadata;
    for (let run = 0; run < 5; run += 1) {
      const sessionStarted = performance.now();
      const metadata = await invoke('viewer_open_test_fixture');
      const sessionMs = performance.now() - sessionStarted;
      const pathLikeFields = Object.entries(metadata)
        .filter(([key, value]) => key !== 'mimeType' && typeof value === 'string'
          && (value.includes(':\\') || value.startsWith('\\\\') || value.includes('/')))
        .map(([key]) => key);
      if ('path' in metadata || pathLikeFields.length > 0) {
        throw new Error(`Viewer metadata leaked a source path field: ${pathLikeFields.join(', ')}`);
      }
      const end = Math.min(metadata.sizeBytes, 256 * 1024);
      const rangeStarted = performance.now();
      const response = await invoke('viewer_read_range', {
        request: {
          sessionId: metadata.sessionId,
          generation: metadata.generation,
          begin: 0,
          end,
        },
      });
      const rangeMs = performance.now() - rangeStarted;
      responseTypes.add(Object.prototype.toString.call(response));
      const bytes = response instanceof Uint8Array ? response : new Uint8Array(response);
      header = new TextDecoder('ascii').decode(bytes.slice(0, 8));
      byteLength = bytes.byteLength;
      metadataKeys = Object.keys(metadata).sort();
      if (!header.startsWith('%PDF-')) throw new Error('Raw Tauri IPC did not return PDF bytes.');
      const closeStarted = performance.now();
      await invoke('viewer_close', {
        request: { sessionId: metadata.sessionId, generation: metadata.generation },
      });
      samples.push({ sessionMs, rangeMs, closeMs: performance.now() - closeStarted });
      lastMetadata = metadata;
    }
    let staleRejected = false;
    try {
      await invoke('viewer_read_range', {
        request: {
          sessionId: lastMetadata.sessionId,
          generation: lastMetadata.generation,
          begin: 0,
          end: Math.min(lastMetadata.sizeBytes, 256 * 1024),
        },
      });
    } catch {
      staleRejected = true;
    }
    return {
      header,
      byteLength,
      bridgeValueTypes: [...responseTypes],
      base64Response: responseTypes.has('[object String]'),
      staleRejected,
      userAgent: navigator.userAgent,
      metadataKeys,
      samples,
    };
  });
  if (!result.staleRejected) throw new Error('A closed viewer session still returned bytes.');
  if (result.base64Response) throw new Error('The raw Tauri response was converted to base64.');
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
} finally {
  await browser.close();
}
