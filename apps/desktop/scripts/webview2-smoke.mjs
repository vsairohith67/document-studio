import { chromium } from 'playwright';
import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

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
    const invokeStage = async (stage, command, body = {}, options) => {
      try {
        return await invoke(command, body, options);
      } catch (reason) {
        throw new Error(`${stage}: ${JSON.stringify(reason)}`);
      }
    };
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
    const systemStatus = await invokeStage('system-status', 'system_status');
    const renderMetadata = await invokeStage('render-session-open', 'viewer_open_test_fixture');
    const destination = await invokeStage(
      'destination-grant',
      'viewer_grant_test_destination',
    );
    const renderSession = await invokeStage('job-create', 'jobs_create_pdf_to_images', {
      request: {
        viewerSessionId: renderMetadata.sessionId,
        viewerGeneration: renderMetadata.generation,
        destinationGrantId: destination.grantId,
        sourcePageCount: 44,
        pages: [{ sourcePageIndex: 0, width: 1, height: 1 }],
        format: 'png',
        dpi: 72,
        outputStem: 'webview2-g04b2',
      },
    });
    const ticket = renderSession.pages[0];
    const headers = {
      'x-document-studio-job-id': renderSession.job.id,
      'x-document-studio-render-session-id': renderSession.renderSessionId,
      'x-document-studio-page-ordinal': String(ticket.pageOrdinal),
      'x-document-studio-page-nonce': ticket.nonce,
      'x-document-studio-expected-width': String(ticket.expectedWidth),
      'x-document-studio-expected-height': String(ticket.expectedHeight),
    };
    const rawPixels = new Uint8Array([12, 34, 56, 255]);
    const rawBodyType = Object.prototype.toString.call(rawPixels);
    const completed = await invokeStage(
      'valid-raw-page-submit',
      'pdf_to_images_submit_page',
      rawPixels,
      { headers },
    );
    let missingHeaderRejected = false;
    try {
      const { 'x-document-studio-page-nonce': _missing, ...incompleteHeaders } = headers;
      await invoke('pdf_to_images_submit_page', new Uint8Array([12, 34, 56, 255]), {
        headers: incompleteHeaders,
      });
    } catch {
      missingHeaderRejected = true;
    }
    let replayRejected = false;
    try {
      await invoke('pdf_to_images_submit_page', new Uint8Array([12, 34, 56, 255]), { headers });
    } catch {
      replayRejected = true;
    }
    let jsonPixelBodyRejected = false;
    try {
      await invoke('pdf_to_images_submit_page', { rgba: [12, 34, 56, 255] }, { headers });
    } catch {
      jsonPixelBodyRejected = true;
    }
    await invokeStage('render-session-close', 'viewer_close', {
      request: { sessionId: renderMetadata.sessionId, generation: renderMetadata.generation },
    });
    await invokeStage(
      'destination-revoke',
      'viewer_revoke_destination',
      { request: { grantId: destination.grantId } },
    );
    const redactedJob = JSON.stringify({ session: renderSession.job, completed });
    const pathRedacted = !redactedJob.includes(':\\')
      && !redactedJob.includes('destinationDirectory":"C:')
      && completed.destinationDirectory === ''
      && completed.inputs.every((input) => input.sourcePath === '' && input.canonicalPath === '')
      && completed.outputs.every((output) => output.stagingPath === null
        && output.partialPath === null && output.finalPath === null);
    return {
      header,
      byteLength,
      bridgeValueTypes: [...responseTypes],
      base64Response: responseTypes.has('[object String]'),
      staleRejected,
      userAgent: navigator.userAgent,
      metadataKeys,
      samples,
      webview2RuntimeVersion: systemStatus.webview2RuntimeVersion,
      g04b2: {
        operationId: completed.operationId,
        operationVersion: completed.operationVersion,
        state: completed.state,
        outputStatus: completed.outputs[0].status,
        rawBodyType,
        missingHeaderRejected,
        replayRejected,
        jsonPixelBodyRejected,
        pathRedacted,
      },
    };
  });
  if (!result.staleRejected) throw new Error('A closed viewer session still returned bytes.');
  if (result.base64Response) throw new Error('The raw Tauri response was converted to base64.');
  if (!result.webview2RuntimeVersion) throw new Error('The WebView2 runtime version was not recorded.');
  if (result.g04b2.operationId !== 'pdf.to-images'
      || result.g04b2.operationVersion !== '1.0.0'
      || result.g04b2.state !== 'completed'
      || result.g04b2.outputStatus !== 'published'
      || result.g04b2.rawBodyType !== '[object Uint8Array]'
      || !result.g04b2.missingHeaderRejected
      || !result.g04b2.replayRejected
      || !result.g04b2.jsonPixelBodyRejected
      || !result.g04b2.pathRedacted) {
    throw new Error(`The native G04B2 raw IPC boundary failed: ${JSON.stringify(result.g04b2)}`);
  }
  const outputDirectory = process.env.DOCUMENT_STUDIO_TEST_OUTPUT_DIRECTORY;
  if (!outputDirectory) throw new Error('The isolated G04B2 output directory was not configured.');
  const outputFiles = await readdir(outputDirectory);
  if (outputFiles.length !== 1 || outputFiles[0] !== 'webview2-g04b2-page-0001.png') {
    throw new Error(`The native G04B2 publication set is not exact: ${outputFiles.join(',')}`);
  }
  const outputBytes = await readFile(resolve(outputDirectory, outputFiles[0]));
  if (!outputBytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) {
    throw new Error('The native G04B2 published output is not PNG content.');
  }
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
} finally {
  await browser.close();
}
