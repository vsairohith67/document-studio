import { describe, expect, it, vi } from 'vitest';
import type { ViewerDocumentMetadata } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({ readRange: vi.fn() }));
vi.mock('../api', () => ({ api: { viewer: { readRange: mocks.readRange } } }));

import {
  MAX_RANGE_READS,
  RANGE_CHUNK_BYTES,
  SessionRangeTransport,
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
});
