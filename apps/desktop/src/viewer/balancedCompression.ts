import {
  MAX_RANGE_READS,
  type BalancedCompressionVisualSession,
  type JobRecord,
  type ViewerDocumentMetadata,
} from '@document-studio/contracts';
import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { api } from '../api';
import {
  createBoundedRangeReader,
  loadPdfSession,
  MAX_CANVAS_HEIGHT,
  MAX_CANVAS_PIXELS,
  MAX_CANVAS_WIDTH,
  SAFE_PAGE_RENDER_OPTIONS,
  type LoadedPdfSession,
  type PdfRangeReader,
} from './pdfSession';

const RENDER_SCALE = 2;
const SIDE_TIMEOUT_MS = 60_000;
const TOTAL_TIMEOUT_MS = 30 * 60_000;

export interface BalancedRenderHooks {
  onRenderTask?: (task: RenderTask | null) => void;
  onJob?: (job: JobRecord) => void;
}

export async function renderBalancedCompression(
  session: BalancedCompressionVisualSession,
  signal: AbortSignal,
  hooks: BalancedRenderHooks = {},
): Promise<JobRecord> {
  const stageController = new AbortController();
  const abortStage = () => stageController.abort();
  if (signal.aborted) abortStage();
  else signal.addEventListener('abort', abortStage, { once: true });
  const totalTimer = window.setTimeout(abortStage, TOTAL_TIMEOUT_MS);
  let source: LoadedPdfSession | null = null;
  let candidate: LoadedPdfSession | null = null;
  const readRange = createBoundedRangeReader(MAX_RANGE_READS);
  try {
    source = await loadOwnedSession(session.source, stageController.signal, readRange);
    candidate = await loadOwnedSession(session.candidate, stageController.signal, readRange);
    if (source.document.numPages !== candidate.document.numPages) {
      throw new Error('The balanced candidate page count changed.');
    }
    let job: JobRecord | null = null;
    for (const ticket of session.pages) {
      const sourcePixels = await renderPage(
        source.document,
        ticket.sourcePageIndex,
        stageController.signal,
        hooks.onRenderTask,
      );
      try {
        job = await api.jobs.submitBalancedPixels(
          session,
          ticket,
          'source',
          sourcePixels.width,
          sourcePixels.height,
          sourcePixels.rgba,
        );
        hooks.onJob?.(job);
      } finally {
        sourcePixels.rgba.fill(0);
      }

      const candidatePixels = await renderPage(
        candidate.document,
        ticket.sourcePageIndex,
        stageController.signal,
        hooks.onRenderTask,
      );
      try {
        if (candidatePixels.width !== sourcePixels.width
            || candidatePixels.height !== sourcePixels.height) {
          throw new Error('The balanced candidate page geometry changed.');
        }
        job = await api.jobs.submitBalancedPixels(
          session,
          ticket,
          'candidate',
          candidatePixels.width,
          candidatePixels.height,
          candidatePixels.rgba,
        );
        hooks.onJob?.(job);
      } finally {
        candidatePixels.rgba.fill(0);
      }
    }
    if (!job) throw new Error('The balanced visual plan contained no affected pages.');
    return job;
  } finally {
    window.clearTimeout(totalTimer);
    signal.removeEventListener('abort', abortStage);
    stageController.abort();
    hooks.onRenderTask?.(null);
    await source?.close().catch(() => undefined);
    await candidate?.close().catch(() => undefined);
    await closeNativeSession(session.source);
    await closeNativeSession(session.candidate);
  }
}

async function loadOwnedSession(
  metadata: ViewerDocumentMetadata,
  signal: AbortSignal,
  readRange: PdfRangeReader,
): Promise<LoadedPdfSession> {
  let encrypted = false;
  const loaded = await loadPdfSession(
    metadata,
    () => { encrypted = true; },
    () => undefined,
    signal,
    undefined,
    readRange,
  );
  if (encrypted) {
    await loaded.close();
    throw new Error('Encrypted PDFs are refused by balanced compression.');
  }
  return loaded;
}

async function renderPage(
  document: PDFDocumentProxy,
  pageIndex: number,
  signal: AbortSignal,
  onRenderTask: ((task: RenderTask | null) => void) | undefined,
): Promise<{ rgba: Uint8Array; width: number; height: number }> {
  if (signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
  const sideStarted = performance.now();
  const remaining = () => Math.max(0, SIDE_TIMEOUT_MS - (performance.now() - sideStarted));
  const page = await withTimeout(document.getPage(pageIndex + 1), signal, remaining());
  const viewport = page.getViewport({ scale: RENDER_SCALE });
  const width = Math.ceil(viewport.width);
  const height = Math.ceil(viewport.height);
  const pixels = width * height;
  if (!Number.isSafeInteger(pixels)
      || width < 1
      || height < 1
      || width > MAX_CANVAS_WIDTH
      || height > MAX_CANVAS_HEIGHT
      || pixels > MAX_CANVAS_PIXELS) {
    page.cleanup();
    throw new Error('An affected page exceeds the fixed balanced render budget.');
  }
  const canvas = window.document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d', {
    alpha: false,
    willReadFrequently: true,
  });
  if (!context) {
    page.cleanup();
    throw new Error('The opaque local page canvas is unavailable.');
  }
  context.save();
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, width, height);
  context.restore();
  const task = page.render({
    canvas,
    canvasContext: context,
    viewport,
    background: '#ffffff',
    annotationMode: SAFE_PAGE_RENDER_OPTIONS.annotationMode,
    intent: 'print',
  });
  onRenderTask?.(task);
  const abort = () => task.cancel();
  signal.addEventListener('abort', abort, { once: true });
  try {
    await withTimeout(task.promise, signal, remaining(), () => task.cancel());
    const image = context.getImageData(0, 0, width, height);
    const rgba = new Uint8Array(image.data);
    if (rgba.length !== pixels * 4) {
      throw new Error('The rendered RGBA8 byte count is invalid.');
    }
    for (let offset = 3; offset < rgba.length; offset += 4) {
      if (rgba[offset] !== 255) {
        throw new Error('The rendered page is not fully opaque.');
      }
    }
    return { rgba, width, height };
  } finally {
    signal.removeEventListener('abort', abort);
    onRenderTask?.(null);
    canvas.width = 0;
    canvas.height = 0;
    page.cleanup();
  }
}

async function withTimeout<T>(
  promise: Promise<T>,
  signal: AbortSignal,
  timeoutMs: number,
  onTimeout: () => void = () => undefined,
): Promise<T> {
  let timer: number | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = window.setTimeout(
      () => {
        onTimeout();
        reject(new Error('The affected page render exceeded 60 seconds.'));
      },
      timeoutMs,
    );
  });
  const abort = () => rejectAbort();
  let abortReject: ((reason: DOMException) => void) | undefined;
  const rejectAbort = () => abortReject?.(new DOMException('Rendering cancelled', 'AbortError'));
  const aborted = new Promise<never>((_, reject) => {
    abortReject = reject;
    if (signal.aborted) {
      rejectAbort();
      return;
    }
    signal.addEventListener('abort', abort, { once: true });
  });
  try {
    return await Promise.race([promise, timeout, aborted]);
  } finally {
    if (timer !== undefined) window.clearTimeout(timer);
    signal.removeEventListener('abort', abort);
  }
}

async function closeNativeSession(metadata: ViewerDocumentMetadata): Promise<void> {
  await api.viewer.close({
    sessionId: metadata.sessionId,
    generation: metadata.generation,
  }).catch(() => undefined);
}
