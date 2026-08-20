import { describe, expect, it, vi } from 'vitest';
import type { ViewerDocumentMetadata } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({ readRange: vi.fn(), getDocument: vi.fn() }));
vi.mock('../api', () => ({ api: { viewer: { readRange: mocks.readRange } } }));
vi.mock('pdfjs-dist/legacy/build/pdf.mjs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('pdfjs-dist/legacy/build/pdf.mjs')>();
  return { ...actual, getDocument: mocks.getDocument };
});

import {
  MAX_RANGE_READS,
  RANGE_CHUNK_BYTES,
  SessionRangeTransport,
  loadPdfSession,
} from './pdfSession';

const session: ViewerDocumentMetadata = {
  sessionId: 'opaque-session', generation: 7, displayName: 'document.pdf',
  sizeBytes: RANGE_CHUNK_BYTES * 6, modifiedAt: '2026-08-17T00:00:00Z',
  mimeType: 'application/pdf', fileIdentity: 'opaque-file-identity',
};

describe('PDF.js opaque range transport', () => {
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
    const transport = new SessionRangeTransport(session, new Uint8Array(), vi.fn());
    transport.transportReady(() => undefined);
    transport.requestDataRange(0, RANGE_CHUNK_BYTES * 6);
    expect(mocks.readRange).toHaveBeenCalledTimes(MAX_RANGE_READS);
    releases.shift()?.();
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(5));
    while (releases.length > 0) releases.shift()?.();
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(6));
    while (releases.length > 0) releases.shift()?.();
    expect(peak).toBe(MAX_RANGE_READS);
    for (const [request] of mocks.readRange.mock.calls) {
      expect(request.end - request.begin).toBeLessThanOrEqual(RANGE_CHUNK_BYTES);
      expect(request).not.toHaveProperty('path');
      expect(request.sessionId).toBe('opaque-session');
    }
    transport.abort();
  });

  it('allows overlapping and repeated requests without a shared cursor', async () => {
    mocks.readRange.mockImplementation(async (request: { begin: number; end: number }) =>
      new Uint8Array(request.end - request.begin).fill(request.begin % 251));
    const transport = new SessionRangeTransport(session, new Uint8Array(), vi.fn());
    transport.transportReady(() => undefined);
    transport.requestDataRange(10, 100);
    transport.requestDataRange(50, 120);
    transport.requestDataRange(10, 100);
    await vi.waitFor(() => expect(mocks.readRange).toHaveBeenCalledTimes(3));
    expect(mocks.readRange.mock.calls.map(([request]) => [request.begin, request.end])).toEqual([
      [10, 100], [50, 120], [10, 100],
    ]);
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
});
