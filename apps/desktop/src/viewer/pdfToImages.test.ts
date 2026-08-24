import { describe, expect, it, vi } from 'vitest';
import type { PDFDocumentProxy, PDFPageProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
import type { PdfToImagesJobSession } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({ submitPdfPixels: vi.fn() }));

vi.mock('../api', () => ({
  api: { jobs: { submitPdfPixels: mocks.submitPdfPixels } },
}));

import {
  PDF_TO_IMAGES_MAX_TOTAL_PIXELS,
  planPdfImagePages,
  plannedCanvasSize,
  renderPdfImageJob,
} from './pdfToImages';

function page(baseWidth = 612, baseHeight = 792): PDFPageProxy {
  return {
    getViewport: vi.fn(({ scale }: { scale: number }) => ({
      width: baseWidth * scale,
      height: baseHeight * scale,
    })),
    cleanup: vi.fn(),
  } as unknown as PDFPageProxy;
}

function document(pages: PDFPageProxy[]): PDFDocumentProxy {
  return {
    numPages: pages.length,
    getPage: vi.fn(async (pageNumber: number) => pages[pageNumber - 1]),
  } as unknown as PDFDocumentProxy;
}

describe('PDF-to-images browser render planning', () => {
  it.each([
    [72, 612, 792],
    [150, 1275, 1650],
    [300, 2550, 3300],
  ] as const)('uses viewport scale DPI / 72 for %i DPI', (dpi, width, height) => {
    expect(plannedCanvasSize(page(), dpi)).toMatchObject({ width, height });
  });

  it('preserves selected subsets and ordering', async () => {
    const pages = [page(100, 200), page(200, 300), page(300, 400)];
    const planned = await planPdfImagePages(document(pages), [2, 0], 72);
    expect(planned).toEqual([
      { sourcePageIndex: 2, width: 300, height: 400 },
      { sourcePageIndex: 0, width: 100, height: 200 },
    ]);
  });

  it('uses the rotated/cropped viewport dimensions returned by PDF.js', () => {
    expect(plannedCanvasSize(page(792, 540), 72)).toMatchObject({ width: 792, height: 540 });
  });

  it('rejects 129 pages before rendering', async () => {
    const pages = Array.from({ length: 129 }, () => page(1, 1));
    await expect(planPdfImagePages(document(pages), pages.map((_, index) => index), 72))
      .rejects.toThrow('1 and 128');
  });

  it.each([
    ['width', 8193, 1],
    ['height', 1, 8193],
    ['pixels', 8192, 8192],
  ])('rejects the %s cap before canvas allocation', (_label, width, height) => {
    expect(() => plannedCanvasSize(page(width, height), 72)).toThrow('render cap');
  });

  it('rejects aggregate work above the canonical 67,108,864-pixel budget', async () => {
    const pages = [page(4096, 4096), page(4096, 4096), page(4096, 4096), page(4096, 4096), page(1, 1)];
    expect(4096 * 4096 * 4).toBe(PDF_TO_IMAGES_MAX_TOTAL_PIXELS);
    await expect(planPdfImagePages(document(pages), [0, 1, 2, 3, 4], 72))
      .rejects.toThrow('aggregate work budget');
  });

  it('cancels RenderTask at the maximum permitted page size and releases the private canvas', async () => {
    let rejectRender!: (reason: unknown) => void;
    const cancel = vi.fn(() => rejectRender(new Error('PDF.js render cancelled')));
    const render = vi.fn(() => ({
      promise: new Promise<void>((_resolve, reject) => { rejectRender = reject; }),
      cancel,
    }));
    const cleanup = vi.fn();
    const maximumPage = {
      getViewport: vi.fn(() => ({ width: 4096, height: 4096 })),
      render,
      cleanup,
    } as unknown as PDFPageProxy;
    const pdf = {
      numPages: 1,
      getPage: vi.fn(async () => maximumPage),
    } as unknown as PDFDocumentProxy;
    const fakeContext = {
      save: vi.fn(), restore: vi.fn(), fillRect: vi.fn(), getImageData: vi.fn(),
      fillStyle: '',
    };
    const fakeCanvas = {
      width: 0, height: 0,
      getContext: vi.fn(() => fakeContext),
    } as unknown as HTMLCanvasElement;
    const nativeCreateElement = window.document.createElement.bind(window.document);
    const createElement = vi.spyOn(window.document, 'createElement')
      .mockImplementation((tagName: string) => (
        tagName === 'canvas' ? fakeCanvas : nativeCreateElement(tagName)
      ));
    const controller = new AbortController();
    const session = {
      job: { id: 'job-id' },
      renderSessionId: 'render-session',
      pages: [{
        pageOrdinal: 0, sourcePageIndex: 0, nonce: 'nonce',
        expectedWidth: 4096, expectedHeight: 4096,
      }],
    } as PdfToImagesJobSession;
    const started = performance.now();
    const running = renderPdfImageJob(pdf, session, 72, controller.signal, vi.fn());
    await vi.waitFor(() => expect(render).toHaveBeenCalledOnce());
    controller.abort();
    await expect(running).rejects.toMatchObject({ name: 'AbortError' });
    expect(performance.now() - started).toBeLessThan(2_000);
    expect(cancel).toHaveBeenCalled();
    expect(cleanup).toHaveBeenCalledOnce();
    expect(fakeCanvas.width).toBe(0);
    expect(fakeCanvas.height).toBe(0);
    expect(mocks.submitPdfPixels).not.toHaveBeenCalled();
    createElement.mockRestore();
  });
});
