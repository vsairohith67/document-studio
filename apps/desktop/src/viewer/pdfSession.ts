import {
  AnnotationMode,
  GlobalWorkerOptions,
  PDFDataRangeTransport,
  PasswordResponses,
  getDocument,
  type PDFDocumentLoadingTask,
  type PDFDocumentProxy,
} from 'pdfjs-dist/legacy/build/pdf.mjs';
import {
  CORE_PDF_MAX_PAGES,
  type OperationError,
  type ViewerDocumentMetadata,
} from '@document-studio/contracts';
import { api } from '../api';

export const PDFJS_VERSION = '6.2.108';
export const RANGE_CHUNK_BYTES = 256 * 1024;
export const MAX_RANGE_BYTES = 1024 * 1024;
export const MAX_RANGE_READS = 4;
export const PAGE_DPR_CAP = 2;
export const THUMBNAIL_DPR_CAP = 1.25;
export const MAX_CANVAS_PIXELS = 16_777_216;
export const MAX_CANVAS_WIDTH = 8_192;
export const MAX_CANVAS_HEIGHT = 8_192;
export const MAX_PAGE_CSS_DIMENSION = 32_767;
export { CORE_PDF_MAX_PAGES };

GlobalWorkerOptions.workerSrc = '/pdfjs/pdf.worker.mjs';
export { PasswordResponses };

type RangeWork = { begin: number; end: number };

export class SessionRangeTransport extends PDFDataRangeTransport {
  readonly session: ViewerDocumentMetadata;
  private readonly queue: RangeWork[] = [];
  private readonly onFatal: (error: unknown) => void;
  private activeReads = 0;
  private stopped = false;

  constructor(
    session: ViewerDocumentMetadata,
    initialData: Uint8Array,
    onFatal: (error: unknown) => void,
  ) {
    super(session.sizeBytes, initialData, initialData.byteLength === session.sizeBytes, session.displayName);
    this.session = session;
    this.onFatal = onFatal;
  }

  override requestDataRange(begin: number, end: number): void {
    if (this.stopped || !Number.isSafeInteger(begin) || !Number.isSafeInteger(end)) return;
    const safeBegin = Math.max(0, begin);
    const safeEnd = Math.min(this.session.sizeBytes, end);
    if (safeEnd <= safeBegin) return;
    for (let offset = safeBegin; offset < safeEnd; offset += RANGE_CHUNK_BYTES) {
      this.queue.push({ begin: offset, end: Math.min(safeEnd, offset + RANGE_CHUNK_BYTES) });
    }
    this.pump();
  }

  override abort(): void {
    this.stopped = true;
    this.queue.length = 0;
  }

  private pump(): void {
    while (!this.stopped && this.activeReads < MAX_RANGE_READS && this.queue.length > 0) {
      const work = this.queue.shift();
      if (!work) return;
      this.activeReads += 1;
      void api.viewer.readRange({
        sessionId: this.session.sessionId,
        generation: this.session.generation,
        begin: work.begin,
        end: work.end,
      }).then((bytes) => {
        if (!this.stopped) this.onDataRange(work.begin, bytes);
      }).catch((error: unknown) => {
        if (!this.stopped) {
          this.abort();
          this.onFatal(error);
        }
      }).finally(() => {
        this.activeReads -= 1;
        this.pump();
      });
    }
  }
}

export interface PdfPasswordChallenge {
  reason: number;
  submit(password: string): void;
}

export interface LoadedPdfSession {
  document: PDFDocumentProxy;
  loadingTask: PDFDocumentLoadingTask;
  transport: SessionRangeTransport;
  close(): Promise<void>;
}

export interface PdfLoadingResources {
  loadingTask: PDFDocumentLoadingTask;
  transport: SessionRangeTransport;
  close(): Promise<void>;
}

function unsupportedPageCountError(): OperationError {
  return {
    code: 'PDF_PAGE_COUNT_UNSUPPORTED',
    title: 'The PDF has an unsupported page count',
    detail: `Document Studio supports PDFs with between 1 and ${CORE_PDF_MAX_PAGES.toLocaleString('en-US')} pages.`,
    stage: 'inspect',
    retryable: false,
  };
}

export function validatePdfPageCount(pageCount: number): number {
  if (!Number.isFinite(pageCount)
      || !Number.isInteger(pageCount)
      || pageCount < 1
      || pageCount > CORE_PDF_MAX_PAGES) {
    throw unsupportedPageCountError();
  }
  return pageCount;
}

export async function loadPdfSession(
  session: ViewerDocumentMetadata,
  onPassword: (challenge: PdfPasswordChallenge) => void,
  onFatalRangeError: (error: unknown) => void,
  signal?: AbortSignal,
  onLoadingResources?: (resources: PdfLoadingResources) => void,
): Promise<LoadedPdfSession> {
  if (signal?.aborted) throw new DOMException('Loading cancelled', 'AbortError');
  const initialEnd = Math.min(session.sizeBytes, RANGE_CHUNK_BYTES);
  const initialData = await api.viewer.readRange({
    sessionId: session.sessionId,
    generation: session.generation,
    begin: 0,
    end: initialEnd,
  });
  const transport = new SessionRangeTransport(session, initialData, onFatalRangeError);
  if (signal?.aborted) {
    transport.abort();
    throw new DOMException('Loading cancelled', 'AbortError');
  }
  const documentOptions = {
    range: transport,
    rangeChunkSize: RANGE_CHUNK_BYTES,
    disableRange: false,
    disableStream: true,
    disableAutoFetch: true,
    cMapUrl: '/pdfjs/cmaps/',
    cMapPacked: true,
    standardFontDataUrl: '/pdfjs/standard_fonts/',
    iccUrl: '/pdfjs/iccs/',
    wasmUrl: '/pdfjs/wasm/',
    useWorkerFetch: true,
    useWasm: true,
    useSystemFonts: false,
    enableXfa: false,
    renderForms: false,
    stopAtErrors: true,
    maxImageSize: 50_000_000,
    canvasMaxAreaInBytes: 64 * 1024 * 1024,
    fontExtraProperties: false,
    // PDF.js 6 no longer branches on this former evaluator flag. Keeping the
    // explicit false documents the boundary; CSP also omits unsafe-eval.
    isEvalSupported: false,
  } satisfies NonNullable<Parameters<typeof getDocument>[0]>
    & { isEvalSupported: false; renderForms: false };
  const loadingTask = getDocument(documentOptions);
  let alive = true;
  let closed = false;
  loadingTask.onPassword = (updatePassword: (password: string) => void, reason: number) => {
    if (!alive || signal?.aborted) return;
    onPassword({
      reason,
      submit(password: string) {
        if (alive && !signal?.aborted) updatePassword(password);
      },
    });
  };
  const closeLoadingResources = async () => {
    if (closed) return;
    closed = true;
    alive = false;
    loadingTask.onPassword = () => undefined;
    transport.abort();
    signal?.removeEventListener('abort', onAbort);
    await loadingTask.destroy().catch(() => undefined);
  };
  const onAbort = () => { void closeLoadingResources(); };
  signal?.addEventListener('abort', onAbort, { once: true });
  onLoadingResources?.({ loadingTask, transport, close: closeLoadingResources });
  let document: PDFDocumentProxy;
  try {
    document = await loadingTask.promise;
    if (signal?.aborted) throw new DOMException('Loading cancelled', 'AbortError');
    validatePdfPageCount(document.numPages);
  } catch (error) {
    await closeLoadingResources();
    throw error;
  } finally {
    signal?.removeEventListener('abort', onAbort);
  }
  return {
    document,
    loadingTask,
    transport,
    async close() {
      await closeLoadingResources();
    },
  };
}

export const SAFE_PAGE_RENDER_OPTIONS = {
  annotationMode: AnnotationMode.DISABLE,
  renderForms: false,
} as const;
