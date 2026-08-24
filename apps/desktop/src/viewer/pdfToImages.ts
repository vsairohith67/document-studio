import type {
  JobRecord,
  PdfToImagesJobSession,
  PdfToImagesPagePlan,
} from '@document-studio/contracts';
import type { PDFDocumentProxy, PDFPageProxy, RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { api } from '../api';
import {
  MAX_CANVAS_HEIGHT,
  MAX_CANVAS_PIXELS,
  MAX_CANVAS_WIDTH,
  SAFE_PAGE_RENDER_OPTIONS,
} from './pdfSession';

export const PDF_TO_IMAGES_MAX_OUTPUTS = 128;
export const PDF_TO_IMAGES_MAX_TOTAL_PIXELS = 67_108_864;
export const PDF_TO_IMAGES_DPI = [72, 150, 300] as const;

export function plannedCanvasSize(page: PDFPageProxy, dpi: 72 | 150 | 300) {
  const viewport = page.getViewport({ scale: dpi / 72 });
  // PDF point ratios such as 150/72 can land a few ulps above an exact
  // integer. Remove only that floating-point noise before ceiling.
  const width = Math.ceil(viewport.width - 1e-7);
  const height = Math.ceil(viewport.height - 1e-7);
  const pixels = width * height;
  if (!Number.isFinite(viewport.width) || !Number.isFinite(viewport.height)
    || !Number.isSafeInteger(width) || !Number.isSafeInteger(height)
    || width < 1 || height < 1
    || width > MAX_CANVAS_WIDTH || height > MAX_CANVAS_HEIGHT
    || !Number.isSafeInteger(pixels) || pixels > MAX_CANVAS_PIXELS) {
    throw new Error('A selected page exceeds the 8,192-axis or 16,777,216-pixel render cap at this DPI.');
  }
  return { viewport, width, height, pixels };
}

export async function planPdfImagePages(
  document: PDFDocumentProxy,
  selectedOrder: readonly number[],
  dpi: 72 | 150 | 300,
  signal?: AbortSignal,
): Promise<PdfToImagesPagePlan[]> {
  if (selectedOrder.length < 1 || selectedOrder.length > PDF_TO_IMAGES_MAX_OUTPUTS
    || new Set(selectedOrder).size !== selectedOrder.length) {
    throw new Error('Choose between 1 and 128 unique PDF pages.');
  }
  const plan: PdfToImagesPagePlan[] = [];
  let totalPixels = 0;
  for (const sourcePageIndex of selectedOrder) {
    if (signal?.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
    if (!Number.isSafeInteger(sourcePageIndex) || sourcePageIndex < 0 || sourcePageIndex >= document.numPages) {
      throw new Error('The selected page order no longer matches the open PDF.');
    }
    const page = await document.getPage(sourcePageIndex + 1);
    try {
      const { width, height, pixels } = plannedCanvasSize(page, dpi);
      totalPixels += pixels;
      if (!Number.isSafeInteger(totalPixels) || totalPixels > PDF_TO_IMAGES_MAX_TOTAL_PIXELS) {
        throw new Error('The selected pages exceed the 67,108,864-pixel aggregate work budget. Reduce the page count or DPI.');
      }
      plan.push({ sourcePageIndex, width, height });
    } finally {
      page.cleanup();
    }
  }
  return plan;
}

export async function renderPdfImageJob(
  document: PDFDocumentProxy,
  session: PdfToImagesJobSession,
  dpi: 72 | 150 | 300,
  signal: AbortSignal,
  onPage: (job: JobRecord) => void,
  onRenderTask?: (task: RenderTask | null) => void,
): Promise<JobRecord> {
  let latest = session.job;
  for (const ticket of session.pages) {
    if (signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
    const page = await document.getPage(ticket.sourcePageIndex + 1);
    const canvas = window.document.createElement('canvas');
    let renderTask: RenderTask | null = null;
    const cancelRender = () => renderTask?.cancel();
    try {
      const { viewport, width, height } = plannedCanvasSize(page, dpi);
      if (width !== ticket.expectedWidth || height !== ticket.expectedHeight) {
        throw new Error('The authenticated render dimensions no longer match the page plan.');
      }
      canvas.width = width;
      canvas.height = height;
      const context = canvas.getContext('2d', { alpha: false, willReadFrequently: true });
      if (!context) throw new Error('The private PDF render canvas is unavailable.');
      context.save();
      context.fillStyle = '#ffffff';
      context.fillRect(0, 0, width, height);
      context.restore();
      renderTask = page.render({
        canvas,
        canvasContext: context,
        viewport,
        annotationMode: SAFE_PAGE_RENDER_OPTIONS.annotationMode,
        background: '#ffffff',
        intent: 'print',
      });
      onRenderTask?.(renderTask);
      signal.addEventListener('abort', cancelRender, { once: true });
      try {
        await renderTask.promise;
      } catch (reason) {
        if (signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
        throw reason;
      }
      if (signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
      // getImageData is synchronous and cannot be interrupted. Cancellation is
      // checked immediately before and after this bounded maximum-size read.
      const rgba = context.getImageData(0, 0, width, height).data;
      if (signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
      latest = await api.jobs.submitPdfPixels(
        session,
        ticket,
        new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength),
      );
      onPage(latest);
    } finally {
      signal.removeEventListener('abort', cancelRender);
      onRenderTask?.(null);
      renderTask?.cancel();
      page.cleanup();
      canvas.width = 0;
      canvas.height = 0;
    }
  }
  return latest;
}
