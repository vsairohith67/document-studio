import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  BalancedCompressionVisualSession,
  JobRecord,
  ViewerDocumentMetadata,
} from '@document-studio/contracts';
import type { PDFDocumentProxy, PDFPageProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';

const mocks = vi.hoisted(() => ({
  loadPdfSession: vi.fn(),
  submitBalancedPixels: vi.fn(),
  closeNative: vi.fn(),
}));

vi.mock('./pdfSession', () => ({
  createBoundedRangeReader: vi.fn(() => vi.fn()),
  loadPdfSession: mocks.loadPdfSession,
  MAX_CANVAS_HEIGHT: 8192,
  MAX_CANVAS_PIXELS: 16_777_216,
  MAX_CANVAS_WIDTH: 8192,
  SAFE_PAGE_RENDER_OPTIONS: { annotationMode: 1 },
}));

vi.mock('../api', () => ({
  api: {
    jobs: { submitBalancedPixels: mocks.submitBalancedPixels },
    viewer: { close: mocks.closeNative },
  },
}));

import { renderBalancedCompression } from './balancedCompression';

function metadata(id: string): ViewerDocumentMetadata {
  return {
    sessionId: id,
    generation: 1,
    sizeBytes: 1_000,
    fileIdentity: `${id}-identity`,
    displayName: `${id}.pdf`,
    modifiedAt: '2026-08-26T00:00:00Z',
    mimeType: 'application/pdf',
  };
}

function job(state: JobRecord['state']): JobRecord {
  return { id: 'balanced-job', state } as JobRecord;
}

function page(renderValue: number, setRenderValue: (value: number) => void): PDFPageProxy {
  return {
    getViewport: vi.fn(({ scale }: { scale: number }) => ({
      width: 16 * scale,
      height: 16 * scale,
    })),
    render: vi.fn(() => {
      setRenderValue(renderValue);
      return { promise: Promise.resolve(), cancel: vi.fn() };
    }),
    cleanup: vi.fn(),
  } as unknown as PDFPageProxy;
}

function document(pageValue: PDFPageProxy): PDFDocumentProxy {
  return {
    numPages: 1,
    getPage: vi.fn(async () => pageValue),
  } as unknown as PDFDocumentProxy;
}

function session(): BalancedCompressionVisualSession {
  return {
    jobId: 'balanced-job',
    renderSessionId: 'render-session',
    source: metadata('source-session'),
    candidate: metadata('candidate-session'),
    pages: [{ pageOrdinal: 0, sourcePageIndex: 0, nonce: 'page-nonce' }],
    selectedImageCount: 1,
    skippedImageCount: 0,
  };
}

describe('balanced-compression authoritative browser rendering', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('renders source then candidate at scale 2 on opaque canvases and submits raw RGBA sequentially', async () => {
    let renderValue = 0;
    const sourceClose = vi.fn(async () => undefined);
    const candidateClose = vi.fn(async () => undefined);
    mocks.loadPdfSession
      .mockResolvedValueOnce({ document: document(page(60, (value) => { renderValue = value; })), close: sourceClose })
      .mockResolvedValueOnce({ document: document(page(62, (value) => { renderValue = value; })), close: candidateClose });
    const submitted: Array<{ side: string; width: number; height: number; rgba: Uint8Array }> = [];
    mocks.submitBalancedPixels
      .mockImplementationOnce(async (_session, _ticket, side, width, height, rgba) => {
        submitted.push({ side, width, height, rgba: rgba.slice() });
        return job('verifying');
      })
      .mockImplementationOnce(async (_session, _ticket, side, width, height, rgba) => {
        submitted.push({ side, width, height, rgba: rgba.slice() });
        return job('completed');
      });
    mocks.closeNative.mockResolvedValue(undefined);
    const context = {
      save: vi.fn(),
      restore: vi.fn(),
      fillRect: vi.fn(),
      fillStyle: '',
      getImageData: vi.fn((_x: number, _y: number, width: number, height: number) => {
        const data = new Uint8ClampedArray(width * height * 4);
        for (let offset = 0; offset < data.length; offset += 4) {
          data[offset] = renderValue;
          data[offset + 1] = renderValue;
          data[offset + 2] = renderValue;
          data[offset + 3] = 255;
        }
        return { data };
      }),
    };
    const canvases: Array<{ width: number; height: number }> = [];
    const nativeCreateElement = window.document.createElement.bind(window.document);
    const createElement = vi.spyOn(window.document, 'createElement')
      .mockImplementation((tagName: string) => {
        if (tagName !== 'canvas') return nativeCreateElement(tagName);
        const canvas = { width: 0, height: 0, getContext: vi.fn(() => context) };
        canvases.push(canvas);
        return canvas as unknown as HTMLCanvasElement;
      });

    const completed = await renderBalancedCompression(
      session(),
      new AbortController().signal,
    );
    expect(completed.state).toBe('completed');
    expect(submitted.map((entry) => entry.side)).toEqual(['source', 'candidate']);
    expect(submitted.map((entry) => [entry.width, entry.height])).toEqual([[32, 32], [32, 32]]);
    expect(submitted[0].rgba[0]).toBe(60);
    expect(submitted[1].rgba[0]).toBe(62);
    expect(submitted.every((entry) => entry.rgba[3] === 255)).toBe(true);
    expect(sourceClose).toHaveBeenCalledOnce();
    expect(candidateClose).toHaveBeenCalledOnce();
    expect(mocks.closeNative).toHaveBeenCalledTimes(2);
    expect(canvases.every((canvas) => canvas.width === 0 && canvas.height === 0)).toBe(true);
    createElement.mockRestore();
  });

  it('rejects mismatched source and candidate page geometry before candidate upload', async () => {
    let renderValue = 0;
    const sourcePage = page(60, (value) => { renderValue = value; });
    const candidatePage = page(62, (value) => { renderValue = value; });
    candidatePage.getViewport = vi.fn(() => ({ width: 30, height: 32 })) as never;
    mocks.loadPdfSession
      .mockResolvedValueOnce({ document: document(sourcePage), close: vi.fn(async () => undefined) })
      .mockResolvedValueOnce({ document: document(candidatePage), close: vi.fn(async () => undefined) });
    mocks.submitBalancedPixels.mockResolvedValue(job('verifying'));
    mocks.closeNative.mockResolvedValue(undefined);
    const context = {
      save: vi.fn(), restore: vi.fn(), fillRect: vi.fn(), fillStyle: '',
      getImageData: vi.fn((_x: number, _y: number, width: number, height: number) => ({
        data: new Uint8ClampedArray(width * height * 4).fill(255),
      })),
    };
    const nativeCreateElement = window.document.createElement.bind(window.document);
    const createElement = vi.spyOn(window.document, 'createElement')
      .mockImplementation((tagName: string) => (
        tagName === 'canvas'
          ? ({ width: 0, height: 0, getContext: vi.fn(() => context) } as unknown as HTMLCanvasElement)
          : nativeCreateElement(tagName)
      ));
    await expect(renderBalancedCompression(session(), new AbortController().signal))
      .rejects.toThrow('geometry changed');
    expect(mocks.submitBalancedPixels).toHaveBeenCalledTimes(1);
    createElement.mockRestore();
  });

  it('zeroes the submitted raw pixels even when the native upload rejects', async () => {
    let renderValue = 0;
    mocks.loadPdfSession
      .mockResolvedValueOnce({ document: document(page(60, (value) => { renderValue = value; })), close: vi.fn(async () => undefined) })
      .mockResolvedValueOnce({ document: document(page(62, (value) => { renderValue = value; })), close: vi.fn(async () => undefined) });
    let submitted: Uint8Array | null = null;
    mocks.submitBalancedPixels.mockImplementationOnce(async (
      _session, _ticket, _side, _width, _height, rgba: Uint8Array,
    ) => {
      submitted = rgba;
      throw new Error('native upload rejected');
    });
    mocks.closeNative.mockResolvedValue(undefined);
    const context = {
      save: vi.fn(), restore: vi.fn(), fillRect: vi.fn(), fillStyle: '',
      getImageData: vi.fn((_x: number, _y: number, width: number, height: number) => {
        const data = new Uint8ClampedArray(width * height * 4);
        for (let offset = 0; offset < data.length; offset += 4) {
          data[offset] = renderValue;
          data[offset + 1] = renderValue;
          data[offset + 2] = renderValue;
          data[offset + 3] = 255;
        }
        return { data };
      }),
    };
    const nativeCreateElement = window.document.createElement.bind(window.document);
    const createElement = vi.spyOn(window.document, 'createElement')
      .mockImplementation((tagName: string) => (
        tagName === 'canvas'
          ? ({ width: 0, height: 0, getContext: vi.fn(() => context) } as unknown as HTMLCanvasElement)
          : nativeCreateElement(tagName)
      ));
    await expect(renderBalancedCompression(session(), new AbortController().signal))
      .rejects.toThrow('native upload rejected');
    expect(submitted).not.toBeNull();
    expect(Array.from(submitted ?? []).every((value) => value === 0)).toBe(true);
    createElement.mockRestore();
  });

  it('aborts the entire visual stage at the exact 30-minute boundary', async () => {
    vi.useFakeTimers();
    mocks.loadPdfSession.mockImplementation(
      async (_metadata, _onEncrypted, _onProgress, signal: AbortSignal) => (
        await new Promise((_, reject) => {
          signal.addEventListener(
            'abort',
            () => reject(new DOMException('Rendering cancelled', 'AbortError')),
            { once: true },
          );
        })
      ),
    );
    mocks.closeNative.mockResolvedValue(undefined);
    const rendering = renderBalancedCompression(session(), new AbortController().signal);
    const rejection = expect(rendering).rejects.toMatchObject({ name: 'AbortError' });
    await vi.advanceTimersByTimeAsync(30 * 60_000 - 1);
    expect(mocks.closeNative).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await rejection;
    expect(mocks.closeNative).toHaveBeenCalledTimes(2);
  });
});
