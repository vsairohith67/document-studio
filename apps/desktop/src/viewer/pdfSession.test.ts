import { describe, expect, it, vi } from 'vitest';
import type { ViewerDocumentMetadata } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({ readRange: vi.fn(), getDocument: vi.fn() }));
vi.mock('../api', () => ({ api: { viewer: { readRange: mocks.readRange } } }));
vi.mock('pdfjs-dist/legacy/build/pdf.mjs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('pdfjs-dist/legacy/build/pdf.mjs')>();
  return { ...actual, getDocument: mocks.getDocument };
});

import {
  CORE_PDF_MAX_PAGES,
  MAX_QUEUED_RANGE_BYTES,
  MAX_QUEUED_RANGE_COUNT,
  MAX_RANGE_READS,
  RANGE_CHUNK_BYTES,
  SessionRangeTransport,
  createBoundedRangeReader,
  loadPdfSession,
} from './pdfSession';

const session: ViewerDocumentMetadata = {
  sessionId: 'opaque-session', generation: 7, displayName: 'document.pdf',
  sizeBytes: RANGE_CHUNK_BYTES * 6, modifiedAt: '2026-08-17T00:00:00Z',
  mimeType: 'application/pdf', fileIdentity: 'opaque-file-identity',
};

interface DeliveredRange {
  begin: number;
  bytes: Uint8Array;
}

function sessionWithSize(sizeBytes: number, sessionId = 'opaque-session'): ViewerDocumentMetadata {
  return { ...session, sessionId, sizeBytes };
}

function bytesForRange(begin: number, end: number): Uint8Array {
  return Uint8Array.from(
    { length: end - begin },
    (_value, index) => (begin + index) % 251,
  );
}

function readyTransport(
  metadata: ViewerDocumentMetadata = session,
  onFatal = vi.fn(),
): { transport: SessionRangeTransport; deliveries: DeliveredRange[]; onFatal: ReturnType<typeof vi.fn> } {
  const transport = new SessionRangeTransport(metadata, new Uint8Array(), onFatal);
  const deliveries: DeliveredRange[] = [];
  transport.transportReady((event: { type: string; begin?: number; chunk?: Uint8Array }) => {
    if (event.type === 'range' && event.begin !== undefined && event.chunk) {
      deliveries.push({ begin: event.begin, bytes: event.chunk });
    }
  });
  return { transport, deliveries, onFatal };
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe('PDF.js opaque range transport', () => {
  it('shares one four-read native ceiling across multiple PDF sessions', async () => {
    const releases: Array<() => void> = [];
    let active = 0;
    let peak = 0;
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise<void>((resolve) => releases.push(resolve));
      active -= 1;
      return new Uint8Array(request.end - request.begin);
    });
    const readRange = createBoundedRangeReader(MAX_RANGE_READS);
    const pending = Array.from({ length: 8 }, (_, index) => readRange({
      sessionId: index < 4 ? 'source-session' : 'candidate-session',
      generation: 1,
      begin: index * 10,
      end: index * 10 + 10,
    }));
    expect(mocks.readRange).toHaveBeenCalledTimes(4);
    for (let index = 0; index < 4; index += 1) releases.shift()?.();
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(8));
    while (releases.length > 0) releases.shift()?.();
    await Promise.all(pending);
    expect(peak).toBe(MAX_RANGE_READS);
  });

  it('splits normal reads into 256 KiB chunks and keeps at most four in flight', async () => {
    const releases: Array<() => void> = [];
    let active = 0;
    let peak = 0;
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise<void>((resolve) => releases.push(resolve));
      active -= 1;
      return new Uint8Array(request.end - request.begin);
    });
    const { transport, deliveries } = readyTransport();
    transport.requestDataRange(0, RANGE_CHUNK_BYTES * 6);
    expect(mocks.readRange).toHaveBeenCalledTimes(MAX_RANGE_READS);
    releases.shift()?.();
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(5));
    while (releases.length > 0) releases.shift()?.();
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(6));
    while (releases.length > 0) releases.shift()?.();
    await vi.waitFor(() => expect(deliveries).toHaveLength(1));
    expect(peak).toBe(MAX_RANGE_READS);
    for (const [request] of mocks.readRange.mock.calls) {
      expect(request.end - request.begin).toBeLessThanOrEqual(RANGE_CHUNK_BYTES);
      expect(request).not.toHaveProperty('path');
      expect(request.sessionId).toBe('opaque-session');
    }
    expect(deliveries[0]).toMatchObject({ begin: 0 });
    expect(deliveries[0]?.bytes).toHaveLength(RANGE_CHUNK_BYTES * 6);
    transport.abort();
  });

  it('reuses overlapping coverage and duplicate physical I/O while completing each logical request once', async () => {
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) =>
      bytesForRange(request.begin, request.end));
    const { transport, deliveries } = readyTransport();
    transport.requestDataRange(10, 100);
    transport.requestDataRange(50, 120);
    transport.requestDataRange(10, 100);
    await vi.waitFor(() => expect(deliveries).toHaveLength(3));
    expect(mocks.readRange.mock.calls.map(([request]) => [request.begin, request.end])).toEqual([
      [10, 100], [100, 120],
    ]);
    expect(deliveries.map(({ begin, bytes }) => [begin, bytes.byteLength])).toEqual([
      [10, 90], [50, 70], [10, 90],
    ]);
    expect(Array.from(deliveries[1]?.bytes ?? [])).toEqual(
      Array.from(bytesForRange(50, 120)),
    );
    transport.abort();
  });

  it('serves sparse reads and many small ranges in FIFO physical order', async () => {
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) =>
      bytesForRange(request.begin, request.end));
    const { transport, deliveries } = readyTransport(sessionWithSize(RANGE_CHUNK_BYTES * 8));
    const requests = Array.from({ length: 32 }, (_value, index) => {
      const begin = index * 97;
      return { begin, end: begin + 17 };
    });
    requests.push(
      { begin: RANGE_CHUNK_BYTES * 4 + 11, end: RANGE_CHUNK_BYTES * 4 + 43 },
      { begin: RANGE_CHUNK_BYTES * 7 + 3, end: RANGE_CHUNK_BYTES * 7 + 29 },
    );

    for (const request of requests) transport.requestDataRange(request.begin, request.end);

    await vi.waitFor(() => expect(deliveries).toHaveLength(requests.length));
    expect(mocks.readRange.mock.calls.map(([request]) => [request.begin, request.end])).toEqual(
      requests.map(({ begin, end }) => [begin, end]),
    );
    expect(deliveries.map(({ begin }) => begin)).toEqual(requests.map(({ begin }) => begin));
    transport.abort();
  });

  it('rejects a huge interval atomically with a typed path-free overload error', () => {
    mocks.readRange.mockImplementation(() => new Promise<Uint8Array>(() => undefined));
    const { transport, onFatal } = readyTransport(
      sessionWithSize(MAX_QUEUED_RANGE_BYTES + 1),
    );

    transport.requestDataRange(0, MAX_QUEUED_RANGE_BYTES + 1);

    expect(mocks.readRange).not.toHaveBeenCalled();
    expect(onFatal).toHaveBeenCalledTimes(1);
    expect(onFatal.mock.calls[0]?.[0]).toMatchObject({
      code: 'PDF_RANGE_QUEUE_LIMIT_EXCEEDED',
      stage: 'inspect',
      retryable: false,
    });
    const serialized = JSON.stringify(onFatal.mock.calls[0]?.[0]);
    expect(serialized).not.toContain('opaque-session');
    expect(serialized).not.toContain('sessionId');
    expect(serialized).not.toContain('begin');
    expect(serialized).not.toContain('end');
    expect(serialized).not.toContain('path');
  });

  it('sanitizes native range failures before they reach the fatal error channel', async () => {
    mocks.readRange.mockRejectedValue(
      new Error('C:\\Users\\private-owner\\secret-document.pdf at offset 4096'),
    );
    const { transport, onFatal } = readyTransport();

    transport.requestDataRange(0, 32);

    await vi.waitFor(() => expect(onFatal).toHaveBeenCalledTimes(1));
    expect(onFatal.mock.calls[0]?.[0]).toMatchObject({
      code: 'PDF_RANGE_READ_FAILED',
      stage: 'inspect',
      retryable: false,
    });
    const serialized = JSON.stringify(onFatal.mock.calls[0]?.[0]);
    expect(serialized).not.toContain('private-owner');
    expect(serialized).not.toContain('secret-document');
    expect(serialized).not.toContain('4096');
    expect(serialized).not.toContain('path');
    transport.abort();
  });

  it('counts exact duplicate logical requests separately and rejects count plus one', () => {
    mocks.readRange.mockImplementation(() => new Promise<Uint8Array>(() => undefined));
    const { transport, onFatal } = readyTransport();

    for (let index = 0; index < MAX_QUEUED_RANGE_COUNT; index += 1) {
      transport.requestDataRange(0, 1);
    }

    expect(onFatal).not.toHaveBeenCalled();
    expect(mocks.readRange).toHaveBeenCalledTimes(1);
    transport.requestDataRange(0, 1);
    expect(onFatal).toHaveBeenCalledTimes(1);
    expect(onFatal.mock.calls[0]?.[0]).toMatchObject({
      code: 'PDF_RANGE_QUEUE_LIMIT_EXCEEDED',
    });
  });

  it('admits the exact byte limit and rejects bytes plus one before any partial admission', () => {
    mocks.readRange.mockImplementation(() => new Promise<Uint8Array>(() => undefined));
    const { transport, onFatal } = readyTransport(
      sessionWithSize(MAX_QUEUED_RANGE_BYTES + 1),
    );

    transport.requestDataRange(0, MAX_QUEUED_RANGE_BYTES);

    expect(onFatal).not.toHaveBeenCalled();
    expect(mocks.readRange).toHaveBeenCalledTimes(MAX_RANGE_READS);
    for (const [request] of mocks.readRange.mock.calls) {
      expect(request.end - request.begin).toBeLessThanOrEqual(RANGE_CHUNK_BYTES);
    }
    transport.requestDataRange(MAX_QUEUED_RANGE_BYTES, MAX_QUEUED_RANGE_BYTES + 1);
    expect(onFatal).toHaveBeenCalledTimes(1);
    expect(mocks.readRange).toHaveBeenCalledTimes(MAX_RANGE_READS);
  });

  it('reuses uncovered physical spans during an overlap storm at the exact queue limits', async () => {
    const releases = new Map<number, (bytes: Uint8Array) => void>();
    mocks.readRange.mockImplementation((request: { begin: number; end: number }) =>
      new Promise<Uint8Array>((resolve) => releases.set(request.begin, resolve)));
    const { transport, deliveries, onFatal } = readyTransport(
      sessionWithSize(RANGE_CHUNK_BYTES * 2),
    );

    for (let index = 0; index < MAX_QUEUED_RANGE_COUNT / 2; index += 1) {
      transport.requestDataRange(0, RANGE_CHUNK_BYTES);
    }
    for (let index = 0; index < MAX_QUEUED_RANGE_COUNT / 2; index += 1) {
      transport.requestDataRange(RANGE_CHUNK_BYTES / 2, RANGE_CHUNK_BYTES * 1.5);
    }

    expect(onFatal).not.toHaveBeenCalled();
    expect(mocks.readRange.mock.calls.map(([request]) => [request.begin, request.end])).toEqual([
      [0, RANGE_CHUNK_BYTES],
      [RANGE_CHUNK_BYTES, RANGE_CHUNK_BYTES * 1.5],
    ]);
    for (const [begin, release] of releases) {
      const request = mocks.readRange.mock.calls.find(([candidate]) => candidate.begin === begin)?.[0];
      if (request) release(bytesForRange(request.begin, request.end));
    }
    await vi.waitFor(() => expect(deliveries).toHaveLength(MAX_QUEUED_RANGE_COUNT));
    expect(deliveries.slice(0, MAX_QUEUED_RANGE_COUNT / 2).every(
      ({ begin, bytes }) => begin === 0 && bytes.byteLength === RANGE_CHUNK_BYTES,
    )).toBe(true);
    expect(deliveries.slice(MAX_QUEUED_RANGE_COUNT / 2).every(
      ({ begin, bytes }) => begin === RANGE_CHUNK_BYTES / 2
        && bytes.byteLength === RANGE_CHUNK_BYTES,
    )).toBe(true);
    transport.abort();
  });

  it('withholds later logical completions until earlier requests make FIFO progress', async () => {
    const releases = new Map<number, (bytes: Uint8Array) => void>();
    mocks.readRange.mockImplementation((request: { begin: number; end: number }) =>
      new Promise<Uint8Array>((resolve) => releases.set(request.begin, resolve)));
    const { transport, deliveries } = readyTransport();
    transport.requestDataRange(0, 10);
    transport.requestDataRange(20, 30);

    releases.get(20)?.(bytesForRange(20, 30));
    await flushPromises();
    expect(deliveries).toHaveLength(0);

    releases.get(0)?.(bytesForRange(0, 10));
    await vi.waitFor(() => expect(deliveries).toHaveLength(2));
    expect(deliveries.map(({ begin }) => begin)).toEqual([0, 20]);
    transport.abort();
  });

  it.each(['abort', 'destroy'] as const)(
    '%s clears queued accounting and suppresses stale active completions',
    async (stopMethod) => {
      let release!: (bytes: Uint8Array) => void;
      mocks.readRange.mockImplementation((request: { begin: number; end: number }) =>
        new Promise<Uint8Array>((resolve) => { release = resolve; }));
      const { transport, deliveries, onFatal } = readyTransport();
      transport.requestDataRange(0, 32);

      if (stopMethod === 'abort') transport.abort();
      else transport.destroy();
      release(bytesForRange(0, 32));
      await flushPromises();

      expect(deliveries).toHaveLength(0);
      expect(onFatal).not.toHaveBeenCalled();
    },
  );

  it('prevents an old transport epoch from completing into a replacement document', async () => {
    let releaseOld!: (bytes: Uint8Array) => void;
    mocks.readRange.mockImplementation((request: {
      sessionId: string;
      begin: number;
      end: number;
    }) => {
      if (request.sessionId === 'old-session') {
        return new Promise<Uint8Array>((resolve) => { releaseOld = resolve; });
      }
      return Promise.resolve(bytesForRange(request.begin, request.end));
    });
    const oldTransport = readyTransport(sessionWithSize(64, 'old-session'));
    oldTransport.transport.requestDataRange(0, 32);
    oldTransport.transport.destroy();

    const replacement = readyTransport(sessionWithSize(64, 'replacement-session'));
    replacement.transport.requestDataRange(0, 32);
    await vi.waitFor(() => expect(replacement.deliveries).toHaveLength(1));
    releaseOld(bytesForRange(0, 32));
    await flushPromises();

    expect(oldTransport.deliveries).toHaveLength(0);
    expect(oldTransport.onFatal).not.toHaveBeenCalled();
    expect(replacement.deliveries).toHaveLength(1);
    expect(mocks.readRange.mock.calls.map(([request]) => request.sessionId)).toEqual([
      'old-session', 'replacement-session',
    ]);
    replacement.transport.abort();
  });

  it('releases logical count and byte accounting after every completed batch', async () => {
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) =>
      bytesForRange(request.begin, request.end));
    const { transport, deliveries, onFatal } = readyTransport();

    for (let index = 0; index < MAX_QUEUED_RANGE_COUNT; index += 1) {
      transport.requestDataRange(0, 1);
    }
    await vi.waitFor(() => expect(deliveries).toHaveLength(MAX_QUEUED_RANGE_COUNT));

    for (let index = 0; index < MAX_QUEUED_RANGE_COUNT; index += 1) {
      transport.requestDataRange(0, 1);
    }
    await vi.waitFor(() => expect(deliveries).toHaveLength(MAX_QUEUED_RANGE_COUNT * 2));

    expect(onFatal).not.toHaveBeenCalled();
    expect(mocks.readRange).toHaveBeenCalledTimes(2);
    transport.abort();
  });

  it('destroys the loading task and transport when PDF.js rejects the candidate', async () => {
    mocks.readRange.mockResolvedValue(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
    const task = {
      promise: Promise.reject(new Error('damaged PDF')),
      destroy: vi.fn(async () => undefined),
      onPassword: vi.fn(),
    };
    mocks.getDocument.mockReturnValue(task);
    const abort = vi.spyOn(SessionRangeTransport.prototype, 'abort');
    const onPassword = vi.fn();
    await expect(loadPdfSession(session, onPassword, vi.fn())).rejects.toThrow('damaged PDF');
    expect(task.destroy).toHaveBeenCalledTimes(1);
    expect(abort).toHaveBeenCalledTimes(1);
    task.onPassword(vi.fn(), 1);
    expect(onPassword).not.toHaveBeenCalled();
  });

  it('destroys a pending candidate when its abort signal fires', async () => {
    mocks.readRange.mockResolvedValue(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
    let reject!: (error: Error) => void;
    const promise = new Promise<never>((_resolve, fail) => { reject = fail; });
    const task = { promise, destroy: vi.fn(async () => undefined), onPassword: vi.fn() };
    mocks.getDocument.mockReturnValue(task);
    const controller = new AbortController();
    const loading = loadPdfSession(session, vi.fn(), vi.fn(), controller.signal);
    await vi.waitFor(() => expect(mocks.getDocument).toHaveBeenCalled());
    controller.abort();
    reject(new Error('aborted'));
    await expect(loading).rejects.toThrow('aborted');
    expect(task.destroy).toHaveBeenCalled();
  });

  it('keeps a successful task alive until one idempotent close', async () => {
    mocks.readRange.mockResolvedValue(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
    const document = { numPages: 1 };
    const task = { promise: Promise.resolve(document), destroy: vi.fn(async () => undefined), onPassword: vi.fn() };
    mocks.getDocument.mockReturnValue(task);
    const owned = vi.fn();
    const loaded = await loadPdfSession(session, vi.fn(), vi.fn(), undefined, owned);
    expect(owned).toHaveBeenCalledTimes(1);
    expect(owned.mock.calls[0][0]).toMatchObject({ loadingTask: task });
    expect(task.destroy).not.toHaveBeenCalled();
    await loaded.close();
    await loaded.close();
    expect(task.destroy).toHaveBeenCalledTimes(1);
  });

  it.each([1, 1000, CORE_PDF_MAX_PAGES])(
    'accepts a finite supported %i-page document before returning the session',
    async (pageCount) => {
      mocks.readRange.mockResolvedValue(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
      const document = { numPages: pageCount };
      const task = {
        promise: Promise.resolve(document),
        destroy: vi.fn(async () => undefined),
        onPassword: vi.fn(),
      };
      mocks.getDocument.mockReturnValue(task);

      const loaded = await loadPdfSession(session, vi.fn(), vi.fn());

      expect(loaded.document.numPages).toBe(pageCount);
      expect(task.destroy).not.toHaveBeenCalled();
      await loaded.close();
      expect(task.destroy).toHaveBeenCalledTimes(1);
    },
  );

  it.each([0, 1.5, Number.NaN, Number.POSITIVE_INFINITY, CORE_PDF_MAX_PAGES + 1, Number.MAX_SAFE_INTEGER])(
    'rejects unsupported page count %s and destroys the candidate before it can reach consumers',
    async (pageCount) => {
      mocks.readRange.mockResolvedValue(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
      const document = { numPages: pageCount, getPage: vi.fn() };
      const task = {
        promise: Promise.resolve(document),
        destroy: vi.fn(async () => undefined),
        onPassword: vi.fn(),
      };
      mocks.getDocument.mockReturnValue(task);
      const owned = vi.fn();

      await expect(loadPdfSession(session, vi.fn(), vi.fn(), undefined, owned)).rejects.toMatchObject({
        code: 'PDF_PAGE_COUNT_UNSUPPORTED',
        stage: 'inspect',
        retryable: false,
      });

      expect(owned).toHaveBeenCalledTimes(1);
      expect(task.destroy).toHaveBeenCalledTimes(1);
      expect(document.getPage).not.toHaveBeenCalled();
      expect(document).not.toHaveProperty('path');
    },
  );
});
