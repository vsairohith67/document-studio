import { describe, expect, it, vi } from 'vitest';
import type { JobRecord } from '@document-studio/contracts';
import { PdfToImagesOperation } from './pdfToImagesLifecycle';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function job(id: string, state: JobRecord['state']): JobRecord {
  return {
    id,
    operationId: 'pdf.to-images',
    operationVersion: '1.0.0',
    state,
    stage: state === 'cancelled' ? 'cleanup' : 'execute',
    sequence: state === 'cancelled' ? 4 : 3,
    progress: { completedUnits: 0, totalUnits: 1, unit: 'items' },
    destinationDirectory: '',
    requestedOutputName: 'document',
    resolvedOutputName: null,
    cancellationRequestedAt: state === 'cancelled' ? '2026-08-23T00:00:00Z' : null,
    createdAt: '2026-08-23T00:00:00Z',
    updatedAt: '2026-08-23T00:00:00Z',
    finishedAt: state === 'cancelled' ? '2026-08-23T00:00:00Z' : null,
    version: state === 'cancelled' ? 4 : 3,
    inputs: [],
    outputs: [{
      ordinal: 0,
      requestedName: 'document-page-0001.png',
      resolvedName: null,
      stagingPath: null,
      partialPath: null,
      finalPath: null,
      sizeBytes: null,
      mimeType: 'image/png',
      sha256: null,
      status: 'planned',
      verifiedAt: null,
      publishedAt: null,
    }],
    errors: [],
  };
}

describe('PDF-to-images frontend/native ownership handoff', () => {
  it('reconciles a native job that returns after cancellation, repeatedly and exactly once', async () => {
    for (let attempt = 0; attempt < 32; attempt += 1) {
      const creation = deferred<{ id: string }>();
      const native = {
        state: 'creating' as 'creating' | 'live' | 'cancelled',
        sourceHandleOpen: true,
        renderSessionPresent: false,
        cancellationRegistered: false,
        workspacePresent: false,
        stagingEntries: [] as string[],
        history: [] as string[],
        publishedFiles: new Map([['other-operation.png', 'user-owned']]),
      };
      const cancel = vi.fn(async ({ jobId }: { jobId: string }) => {
        expect(jobId).toBe(`late-job-${attempt}`);
        expect(native.state).toBe('live');
        native.state = 'cancelled';
        native.sourceHandleOpen = false;
        native.renderSessionPresent = false;
        native.cancellationRegistered = false;
        native.workspacePresent = false;
        native.stagingEntries = [];
        native.history.push('PDF_TO_IMAGES_CANCELLED: no unverified image was published');
        return { outcome: 'requested' };
      });
      const get = vi.fn(async ({ jobId }: { jobId: string }) => job(jobId, 'cancelled'));
      const operation = new PdfToImagesOperation({ cancel, get });
      const flow = (async () => {
        const created = await creation.promise;
        native.state = 'live';
        native.cancellationRegistered = true;
        native.workspacePresent = true;
        native.renderSessionPresent = true;
        native.stagingEntries = ['private-staging.bin'];
        operation.registerCreatedJob(created.id);
        return operation.reconcileAfterAbort();
      })();

      await operation.requestCancellation();
      expect(operation.signal.aborted).toBe(true);
      expect(cancel).not.toHaveBeenCalled();
      creation.resolve({ id: `late-job-${attempt}` });

      const terminal = await flow;
      expect(terminal.state).toBe('cancelled');
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(get).toHaveBeenCalledTimes(1);
      expect(native).toMatchObject({
        state: 'cancelled',
        sourceHandleOpen: false,
        renderSessionPresent: false,
        cancellationRegistered: false,
        workspacePresent: false,
        stagingEntries: [],
      });
      expect(native.history.join(' ')).not.toMatch(/[A-Z]:\\|private-staging/iu);
      expect(native.publishedFiles.get('other-operation.png')).toBe('user-owned');
    }
  });

  it('does not create or cancel a native job when cancellation wins before create starts', async () => {
    const cancel = vi.fn();
    const operation = new PdfToImagesOperation({ cancel, get: vi.fn() });
    await operation.requestCancellation();
    expect(operation.signal.aborted).toBe(true);
    expect(operation.hasCreatedJob).toBe(false);
    expect(cancel).not.toHaveBeenCalled();
  });

  it('deduplicates cancellation during rendering and after the job ID is known', async () => {
    const cancelGate = deferred<void>();
    const cancel = vi.fn(async () => {
      await cancelGate.promise;
      return { outcome: 'requested' };
    });
    const get = vi.fn(async ({ jobId }: { jobId: string }) => job(jobId, 'cancelled'));
    const operation = new PdfToImagesOperation({ cancel, get });
    operation.registerCreatedJob('known-job');

    const fromButton = operation.requestCancellation();
    const fromAbortedRender = operation.reconcileAfterAbort();
    expect(operation.signal.aborted).toBe(true);
    expect(cancel).toHaveBeenCalledTimes(1);
    cancelGate.resolve();
    await fromButton;
    const terminal = await fromAbortedRender;
    expect(terminal.state).toBe('cancelled');
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(get).toHaveBeenCalledTimes(1);
  });

  it('rejects silent ownership replacement and leaves success and ordinary failure un-cancelled', async () => {
    const cancel = vi.fn();
    const operation = new PdfToImagesOperation({ cancel, get: vi.fn() });
    operation.registerCreatedJob('first-job');
    expect(() => operation.registerCreatedJob('retry-job')).toThrow(/already owns/i);
    expect(operation.ownsJob('first-job')).toBe(true);
    expect(operation.signal.aborted).toBe(false);
    expect(cancel).not.toHaveBeenCalled();

    const subsequent = new PdfToImagesOperation({ cancel, get: vi.fn() });
    subsequent.registerCreatedJob('subsequent-job');
    expect(subsequent.ownsJob('subsequent-job')).toBe(true);
    expect(subsequent.signal.aborted).toBe(false);
    expect(cancel).not.toHaveBeenCalled();
  });

  it('retains a failed source cleanup until a later reconciliation succeeds', async () => {
    const operation = new PdfToImagesOperation({ cancel: vi.fn(), get: vi.fn() });
    const failedCleanup = operation.startFrontendCleanup(async () => {
      throw new Error('viewer close failed');
    });
    await expect(failedCleanup).rejects.toThrow('viewer close failed');
    await expect(operation.waitForFrontendCleanup()).rejects.toThrow('viewer close failed');

    const retry = vi.fn(async () => undefined);
    await operation.startFrontendCleanup(retry);
    await expect(operation.waitForFrontendCleanup()).resolves.toBeUndefined();
    expect(retry).toHaveBeenCalledTimes(1);
  });
});
