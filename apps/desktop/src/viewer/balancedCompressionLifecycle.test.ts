import { describe, expect, it, vi } from 'vitest';
import type { JobRecord, JobState, ProgressEvent } from '@document-studio/contracts';
import { BalancedCompressionOperation } from './balancedCompressionLifecycle';

function job(
  id: string,
  state: JobState,
  sequence: number,
  completionKind: JobRecord['completionKind'] = null,
): JobRecord {
  const published = state === 'completed' && completionKind === 'published';
  return {
    id,
    operationId: 'pdf.compress-balanced',
    operationVersion: '1.0.0',
    state,
    stage: state === 'publishing' ? 'publish' : state === 'completed' ? 'cleanup' : 'execute',
    sequence,
    progress: { completedUnits: state === 'completed' ? 1 : 0, totalUnits: 1, unit: 'items' },
    destinationDirectory: '',
    requestedOutputName: 'balanced.pdf',
    resolvedOutputName: published ? 'balanced.pdf' : null,
    cancellationRequestedAt: state === 'cancelled' ? '2026-08-26T00:00:00Z' : null,
    createdAt: '2026-08-26T00:00:00Z',
    updatedAt: '2026-08-26T00:00:01Z',
    finishedAt: ['completed', 'failed', 'cancelled', 'interrupted'].includes(state)
      ? '2026-08-26T00:00:01Z'
      : null,
    version: sequence,
    completionKind,
    reason: completionKind === 'no-benefit' ? 'savings-threshold-not-met' : null,
    inputs: [],
    outputs: published ? [{
      ordinal: 0,
      requestedName: 'balanced.pdf',
      resolvedName: 'balanced.pdf',
      stagingPath: null,
      partialPath: null,
      finalPath: 'C:\\output\\balanced.pdf',
      sizeBytes: 70_000,
      mimeType: 'application/pdf',
      sha256: 'b'.repeat(64),
      status: 'published',
      verifiedAt: '2026-08-26T00:00:01Z',
      publishedAt: '2026-08-26T00:00:01Z',
    }] : [],
    errors: [],
  };
}

function progress(id: string, state: JobState, sequence: number): ProgressEvent {
  return {
    schemaVersion: 1,
    sequence,
    emittedAt: '2026-08-26T00:00:01Z',
    jobId: id,
    operationId: 'pdf.compress-balanced',
    state,
    stage: state === 'publishing' ? 'publish' : state === 'completed' ? 'cleanup' : 'execute',
    completedUnits: state === 'completed' ? 1 : 0,
    totalUnits: 1,
    unit: 'items',
    messageCode: 'BALANCED_TEST_PROGRESS',
    message: 'Balanced lifecycle test progress',
    cancellable: state !== 'publishing' && state !== 'completed',
  };
}

describe('balanced compression frontend/native operation ownership', () => {
  it('reconciles a job returned after dispose intent repeatedly without leaking ownership', async () => {
    for (let attempt = 0; attempt < 32; attempt += 1) {
      const jobId = `late-balanced-${attempt}`;
      const native = {
        state: 'creating' as 'creating' | 'live' | 'cancelled',
        candidatePresent: false,
        workspacePresent: false,
        viewerSessions: 0,
        outputPublished: false,
        history: [] as string[],
      };
      const cancel = vi.fn(async ({ jobId: requested }: { jobId: string }) => {
        expect(requested).toBe(jobId);
        expect(native.state).toBe('live');
        native.state = 'cancelled';
        native.candidatePresent = false;
        native.workspacePresent = false;
        native.viewerSessions = 0;
        native.history.push('BALANCED_CANCELLED: owned temporary data reconciled');
        return { outcome: 'requested' as const };
      });
      const get = vi.fn(async () => job(jobId, 'cancelled', 4));
      const operation = new BalancedCompressionOperation(attempt + 1, { cancel, get });
      operation.beginCreate();

      await expect(operation.dispose()).resolves.toBeNull();
      expect(operation.createState).toBe('pending');
      expect(cancel).not.toHaveBeenCalled();

      native.state = 'live';
      native.candidatePresent = true;
      native.workspacePresent = true;
      native.viewerSessions = 2;
      const terminal = await operation.registerCreatedJob(job(jobId, 'queued', 0));

      expect(terminal?.state).toBe('cancelled');
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(get).toHaveBeenCalledTimes(1);
      expect(native).toMatchObject({
        state: 'cancelled',
        candidatePresent: false,
        workspacePresent: false,
        viewerSessions: 0,
        outputPublished: false,
      });
      expect(native.history.join(' ')).not.toMatch(/[A-Z]:\\|private|candidate\.pdf/iu);
    }
  });

  it('deduplicates known-job reconciliation across unmount, render abort, and repeated requests', async () => {
    const cancel = vi.fn(async () => ({ outcome: 'requested' as const }));
    const get = vi.fn(async () => job('known-balanced', 'cancelled', 6));
    const operation = new BalancedCompressionOperation(1, { cancel, get });
    operation.beginCreate();
    await operation.registerCreatedJob(job('known-balanced', 'verifying', 4));

    const first = operation.dispose();
    const second = operation.dispose();
    const third = operation.reconcileOwnedJob();
    const [one, two, three] = await Promise.all([first, second, third]);

    expect(one?.state).toBe('cancelled');
    expect(two?.state).toBe('cancelled');
    expect(three?.state).toBe('cancelled');
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(get).toHaveBeenCalledTimes(1);
    expect(operation.cancellationWasRequested).toBe(true);
  });

  it('refreshes a nonterminal accepted-cancellation snapshot without sending cancellation twice', async () => {
    const cancel = vi.fn(async () => ({ outcome: 'requested' as const }));
    const get = vi.fn()
      .mockResolvedValueOnce(job('upload-active', 'verifying', 5))
      .mockResolvedValueOnce(job('upload-active', 'cancelled', 6));
    const operation = new BalancedCompressionOperation(1, { cancel, get });
    operation.beginCreate();
    await operation.registerCreatedJob(job('upload-active', 'verifying', 4));

    const first = await operation.reconcileOwnedJob();
    expect(first?.state).toBe('verifying');
    expect(operation.canStartVisual('upload-active')).toBe(false);

    const terminal = await operation.reconcileOwnedJob();
    expect(terminal?.state).toBe('cancelled');
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('reconciles the known pre-visual unmount race repeatedly with deterministic scheduling', async () => {
    for (let attempt = 0; attempt < 32; attempt += 1) {
      const jobId = `pre-visual-balanced-${attempt}`;
      const cancel = vi.fn(async () => ({ outcome: 'requested' as const }));
      const get = vi.fn(async () => job(jobId, 'cancelled', 3));
      const operation = new BalancedCompressionOperation(attempt + 1, { cancel, get });
      operation.beginCreate();
      await operation.registerCreatedJob(job(jobId, attempt % 2 === 0 ? 'inspecting' : 'preflight', 1));

      const terminal = await operation.dispose();

      expect(terminal?.state).toBe('cancelled');
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(get).toHaveBeenCalledTimes(1);
    }
  });

  it('does not cancel known terminal or publishing jobs and preserves their outcomes', async () => {
    for (const snapshot of [
      job('published', 'completed', 8, 'published'),
      job('no-benefit', 'completed', 8, 'no-benefit'),
      job('commit-crossed', 'publishing', 7),
    ]) {
      const cancel = vi.fn();
      const operation = new BalancedCompressionOperation(1, { cancel, get: vi.fn() });
      operation.beginCreate();
      await operation.registerCreatedJob(snapshot);
      const result = await operation.dispose();
      expect(result).toBe(snapshot);
      expect(operation.latestState).toBe(snapshot.state);
      expect(cancel).not.toHaveBeenCalled();
      expect(snapshot.outputs.find((output) => output.status === 'published')?.finalPath)
        .toBe(snapshot.completionKind === 'published' ? 'C:\\output\\balanced.pdf' : undefined);
    }
  });

  it('reconciles a cancellation that lost the publication race without deleting the published output', async () => {
    const published = job('too-late', 'completed', 8, 'published');
    const cancel = vi.fn(async () => {
      throw { code: 'CANCEL_TOO_LATE', message: 'publication commit already started' };
    });
    const get = vi.fn(async () => published);
    const operation = new BalancedCompressionOperation(1, { cancel, get });
    operation.beginCreate();
    await operation.registerCreatedJob(job('too-late', 'verifying', 6));

    const terminal = await operation.dispose();

    expect(cancel).toHaveBeenCalledTimes(1);
    expect(get).toHaveBeenCalledTimes(1);
    expect(terminal).toBe(published);
    expect(terminal?.completionKind).toBe('published');
    expect(terminal?.outputs[0]?.finalPath).toBe('C:\\output\\balanced.pdf');
  });

  it('accepts only exact generation/job callbacks and ignores stale state regressions', async () => {
    const operation = new BalancedCompressionOperation(12, { cancel: vi.fn(), get: vi.fn() });
    operation.beginCreate();
    await operation.registerCreatedJob(job('owned', 'queued', 1));
    expect(operation.acceptsCallback(12, 'owned')).toBe(true);
    expect(operation.acceptsCallback(11, 'owned')).toBe(false);
    expect(operation.acceptsCallback(12, 'other')).toBe(false);

    expect(operation.observeProgress(progress('owned', 'verifying', 5))).toBe(true);
    expect(operation.observeProgress(progress('owned', 'running', 4))).toBe(false);
    expect(operation.latestState).toBe('verifying');
    expect(operation.canStartVisual('owned')).toBe(true);

    expect(operation.observeProgress(progress('owned', 'completed', 6))).toBe(true);
    expect(operation.canStartVisual('owned')).toBe(false);
    expect(operation.canBeReplaced).toBe(true);
  });

  it('refuses ownership replacement until the previous operation is terminal', async () => {
    const first = new BalancedCompressionOperation(1, { cancel: vi.fn(), get: vi.fn() });
    first.beginCreate();
    await first.registerCreatedJob(job('first', 'running', 2));
    expect(() => first.registerCreatedJob(job('replacement', 'queued', 0))).toThrow(/already owns/i);
    expect(first.canBeReplaced).toBe(false);

    first.observeJob(job('first', 'completed', 3, 'no-benefit'));
    expect(first.canBeReplaced).toBe(true);
    const second = new BalancedCompressionOperation(2, { cancel: vi.fn(), get: vi.fn() });
    second.beginCreate();
    await second.registerCreatedJob(job('second', 'queued', 0));
    expect(second.ownsJob('second')).toBe(true);
  });
});
