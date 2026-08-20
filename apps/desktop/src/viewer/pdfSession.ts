import {
  AnnotationMode,
  GlobalWorkerOptions,
  PDFDataRangeTransport,
  PasswordResponses,
  getDocument,
  type PDFDocumentLoadingTask,
  type PDFDocumentProxy,
} from 'pdfjs-dist/legacy/build/pdf.mjs';
import type { ViewerDocumentMetadata } from '@document-studio/contracts';
import { api } from '../api';

export const PDFJS_VERSION = '6.2.108';
export const RANGE_CHUNK_BYTES = 256 * 1024;
export const MAX_RANGE_BYTES = 1024 * 1024;
export const MAX_RANGE_READS = 4;
export const PAGE_DPR_CAP = 2;
export const THUMBNAIL_DPR_CAP = 1.25;
export const MAX_CANVAS_PIXELS = 16_000_000;

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

export async function loadPdfSession(
  session: ViewerDocumentMetadata,
  onPassword: (challenge: PdfPasswordChallenge) => void,
  onFatalRangeError: (error: unknown) => void,
  signal?: AbortSignal,
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
    stopAtErrors: true,
    maxImageSize: 50_000_000,
    canvasMaxAreaInBytes: 64 * 1024 * 1024,
    fontExtraProperties: false,
    // PDF.js 6 no longer branches on this former evaluator flag. Keeping the
    // explicit false documents the boundary; CSP also omits unsafe-eval.
    isEvalSupported: false,
  } satisfies NonNullable<Parameters<typeof getDocument>[0]> & { isEvalSupported: false };
  const loadingTask = getDocument(documentOptions);
  loadingTask.onPassword = (updatePassword: (password: string) => void, reason: number) => {
    onPassword({
      reason,
      submit(password: string) {
        updatePassword(password);
      },
    });
  };
  const onAbort = () => {
    transport.abort();
    void loadingTask.destroy();
  };
  signal?.addEventListener('abort', onAbort, { once: true });
  const document = await loadingTask.promise.finally(() => signal?.removeEventListener('abort', onAbort));
  let closed = false;
  return {
    document,
    loadingTask,
    transport,
    async close() {
      if (closed) return;
      closed = true;
      transport.abort();
      await loadingTask.destroy();
    },
  };
}

export const SAFE_PAGE_RENDER_OPTIONS = {
  annotationMode: AnnotationMode.DISABLE,
  renderForms: false,
} as const;
