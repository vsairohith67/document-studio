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
  MAX_QUEUED_RANGE_BYTES,
  MAX_QUEUED_RANGE_COUNT,
  MAX_RANGE_READS,
  RANGE_CHUNK_BYTES,
  type OperationError,
  type ViewerDocumentMetadata,
} from '@document-studio/contracts';
import { api } from '../api';

export const PDFJS_VERSION = '6.2.108';
export const MAX_RANGE_BYTES = 1024 * 1024;
export const PAGE_DPR_CAP = 2;
export const THUMBNAIL_DPR_CAP = 1.25;
export const MAX_CANVAS_PIXELS = 16_777_216;
export const MAX_CANVAS_WIDTH = 8_192;
export const MAX_CANVAS_HEIGHT = 8_192;
export const MAX_PAGE_CSS_DIMENSION = 32_767;
export {
  CORE_PDF_MAX_PAGES,
  MAX_QUEUED_RANGE_BYTES,
  MAX_QUEUED_RANGE_COUNT,
  MAX_RANGE_READS,
  RANGE_CHUNK_BYTES,
};

GlobalWorkerOptions.workerSrc = '/pdfjs/pdf.worker.mjs';
export { PasswordResponses };

type PhysicalRangeState = 'queued' | 'active' | 'complete';

interface PhysicalRangeWork {
  begin: number;
  end: number;
  epoch: number;
  state: PhysicalRangeState;
  data: Uint8Array | null;
  references: number;
}

interface LogicalRangePiece {
  range: PhysicalRangeWork;
  offset: number;
  length: number;
}

interface LogicalRangeRequest {
  begin: number;
  byteLength: number;
  pieces: LogicalRangePiece[];
}

function checkedQueueTotal(current: number, delta: number, maximum: number): number | null {
  if (!Number.isSafeInteger(current)
      || !Number.isSafeInteger(delta)
      || current < 0
      || delta < 0
      || current > maximum
      || delta > maximum - current) {
    return null;
  }
  return current + delta;
}

function rangeQueueLimitError(): OperationError {
  return {
    code: 'PDF_RANGE_QUEUE_LIMIT_EXCEEDED',
    title: 'The PDF range request queue exceeded its safe limit',
    detail: 'Document Studio stopped loading the PDF because its local range request queue exceeded a safe limit.',
    stage: 'inspect',
    retryable: false,
  };
}

function rangeReadFailedError(): OperationError {
  return {
    code: 'PDF_RANGE_READ_FAILED',
    title: 'The PDF range could not be read safely',
    detail: 'Document Studio stopped loading the PDF because a local range read did not complete safely.',
    stage: 'inspect',
    retryable: false,
  };
}

export class SessionRangeTransport extends PDFDataRangeTransport {
  readonly session: ViewerDocumentMetadata;
  private readonly physicalQueue: PhysicalRangeWork[] = [];
  private readonly logicalQueue: LogicalRangeRequest[] = [];
  private readonly onFatal: (error: unknown) => void;
  private ranges: PhysicalRangeWork[] = [];
  private activeReads = 0;
  private admittedRangeCount = 0;
  private admittedRangeBytes = 0;
  private transportEpoch = 0;
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

    const byteLength = safeEnd - safeBegin;
    const nextCount = checkedQueueTotal(
      this.admittedRangeCount,
      1,
      MAX_QUEUED_RANGE_COUNT,
    );
    const nextBytes = checkedQueueTotal(
      this.admittedRangeBytes,
      byteLength,
      MAX_QUEUED_RANGE_BYTES,
    );
    if (nextCount === null || nextBytes === null) {
      this.fail(rangeQueueLimitError());
      return;
    }

    const newRanges = this.createUncoveredRanges(safeBegin, safeEnd);
    const coverage = [...this.ranges, ...newRanges]
      .sort((left, right) => left.begin - right.begin || left.end - right.end);
    const pieces = this.buildLogicalPieces(coverage, safeBegin, safeEnd);
    if (!pieces) {
      this.fail(rangeReadFailedError());
      return;
    }

    for (const piece of pieces) piece.range.references += 1;
    this.ranges.push(...newRanges);
    this.ranges.sort((left, right) => left.begin - right.begin || left.end - right.end);
    this.physicalQueue.push(...newRanges);
    this.logicalQueue.push({ begin: safeBegin, byteLength, pieces });
    this.admittedRangeCount = nextCount;
    this.admittedRangeBytes = nextBytes;

    try {
      this.flushCompletedLogicalRanges();
    } catch {
      this.fail(rangeReadFailedError());
      return;
    }
    this.pump();
  }

  override abort(): void {
    this.stop();
  }

  destroy(): void {
    this.stop();
  }

  private stop(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.transportEpoch += 1;
    this.physicalQueue.length = 0;
    this.logicalQueue.length = 0;
    this.ranges.length = 0;
    this.activeReads = 0;
    this.admittedRangeCount = 0;
    this.admittedRangeBytes = 0;
  }

  private fail(error: unknown): void {
    if (this.stopped) return;
    this.stop();
    this.onFatal(error);
  }

  private createUncoveredRanges(begin: number, end: number): PhysicalRangeWork[] {
    const uncovered: PhysicalRangeWork[] = [];
    let cursor = begin;
    const append = (uncoveredEnd: number) => {
      while (cursor < uncoveredEnd) {
        const chunkEnd = cursor + Math.min(RANGE_CHUNK_BYTES, uncoveredEnd - cursor);
        uncovered.push({
          begin: cursor,
          end: chunkEnd,
          epoch: this.transportEpoch,
          state: 'queued',
          data: null,
          references: 0,
        });
        cursor = chunkEnd;
      }
    };

    for (const range of this.ranges) {
      if (range.end <= cursor) continue;
      if (range.begin >= end) break;
      append(Math.min(range.begin, end));
      if (cursor >= end) break;
      if (range.begin <= cursor) cursor = Math.min(end, Math.max(cursor, range.end));
    }
    append(end);
    return uncovered;
  }

  private buildLogicalPieces(
    coverage: PhysicalRangeWork[],
    begin: number,
    end: number,
  ): LogicalRangePiece[] | null {
    const pieces: LogicalRangePiece[] = [];
    let cursor = begin;
    for (const range of coverage) {
      if (range.end <= cursor) continue;
      if (range.begin > cursor || range.begin >= end) break;
      const pieceEnd = Math.min(range.end, end);
      pieces.push({
        range,
        offset: cursor - range.begin,
        length: pieceEnd - cursor,
      });
      cursor = pieceEnd;
      if (cursor === end) break;
    }
    return cursor === end ? pieces : null;
  }

  private flushCompletedLogicalRanges(): void {
    while (!this.stopped && this.logicalQueue.length > 0) {
      const request = this.logicalQueue[0];
      if (!request || request.pieces.some((piece) => piece.range.data === null)) return;

      const bytes = new Uint8Array(request.byteLength);
      let outputOffset = 0;
      for (const piece of request.pieces) {
        const data = piece.range.data;
        if (!data) return;
        bytes.set(data.subarray(piece.offset, piece.offset + piece.length), outputOffset);
        outputOffset += piece.length;
      }

      this.logicalQueue.shift();
      this.admittedRangeCount -= 1;
      this.admittedRangeBytes -= request.byteLength;
      for (const piece of request.pieces) piece.range.references -= 1;
      this.ranges = this.ranges.filter(
        (range) => range.references > 0 || range.state !== 'complete',
      );
      this.onDataRange(request.begin, bytes);
    }
  }

  private pump(): void {
    while (!this.stopped
        && this.activeReads < MAX_RANGE_READS
        && this.physicalQueue.length > 0) {
      const work = this.physicalQueue.shift();
      if (!work) return;
      const epoch = this.transportEpoch;
      work.epoch = epoch;
      work.state = 'active';
      this.activeReads += 1;
      void api.viewer.readRange({
        sessionId: this.session.sessionId,
        generation: this.session.generation,
        begin: work.begin,
        end: work.end,
      }).then((bytes) => {
        if (!this.isCurrent(work, epoch)) return;
        if (bytes.byteLength !== work.end - work.begin) throw rangeReadFailedError();
        work.data = bytes;
        work.state = 'complete';
        this.flushCompletedLogicalRanges();
      }).catch(() => {
        if (this.isCurrent(work, epoch)) this.fail(rangeReadFailedError());
      }).finally(() => {
        if (this.stopped || this.transportEpoch !== epoch) return;
        this.activeReads -= 1;
        this.pump();
      });
    }
  }

  private isCurrent(work: PhysicalRangeWork, epoch: number): boolean {
    return !this.stopped && work.epoch === epoch && this.transportEpoch === epoch;
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
