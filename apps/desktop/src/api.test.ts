import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { JobRecord, ProgressEvent } from '@document-studio/contracts';

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  open: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: native.open }));

import { api, createProgressReconciler, operationErrorMessage } from './api';

const job = { id: '9e515769-e47b-40dd-a334-ddc661a78d45' } as JobRecord;

function progress(sequence: number): ProgressEvent {
  return {
    schemaVersion: 1,
    sequence,
    emittedAt: '2026-08-16T12:00:00Z',
    jobId: job.id,
    operationId: 'diagnostic.copy',
    state: 'running',
    stage: 'execute',
    completedUnits: sequence,
    totalUnits: 10,
    unit: 'bytes',
    messageCode: 'COPYING_BYTES',
    message: 'Copying and checking the file',
    cancellable: true,
  };
}

describe('typed Tauri API', () => {
  beforeEach(() => {
    native.invoke.mockResolvedValue(undefined);
    native.listen.mockResolvedValue(vi.fn());
    native.open.mockResolvedValue(null);
  });

  it('uses the approved command names and request envelope', async () => {
    const request = {
      operationId: 'diagnostic.copy' as const,
      inputPaths: ['C:\\input\\report.pdf'] as [string],
      destinationDirectory: 'C:\\output',
      requestedOutputName: 'report-copy.pdf',
    };

    await api.files.inspect(request.inputPaths);
    await api.jobs.create(request);
    await api.jobs.cancel({ jobId: job.id });
    await api.jobs.resolveInterrupted({ jobId: job.id });
    await api.history.list({ limit: 8 });

    expect(native.invoke).toHaveBeenNthCalledWith(1, 'files_inspect', {
      request: { paths: request.inputPaths },
    });
    expect(native.invoke).toHaveBeenNthCalledWith(2, 'jobs_create', { request });
    expect(native.invoke).toHaveBeenNthCalledWith(3, 'jobs_cancel', {
      request: { jobId: job.id },
    });
    expect(native.invoke).toHaveBeenNthCalledWith(4, 'jobs_resolve_interrupted', {
      request: { jobId: job.id },
    });
    expect(native.invoke).toHaveBeenNthCalledWith(5, 'history_list', {
      request: { limit: 8 },
    });
  });

  it('uses restricted native dialogs and the versioned event name', async () => {
    native.open.mockResolvedValueOnce([
      'C:\\input\\cover.pdf',
      'C:\\input\\body.pdf',
    ]);
    expect(await api.dialogs.selectPdfInputs()).toEqual([
      'C:\\input\\cover.pdf',
      'C:\\input\\body.pdf',
    ]);
    expect(native.open).toHaveBeenNthCalledWith(1, {
      directory: false,
      multiple: true,
      filters: [{ name: 'PDF documents', extensions: ['pdf'] }],
    });

    native.open.mockResolvedValueOnce('C:\\input\\report.pdf');
    expect(await api.dialogs.selectInput()).toBe('C:\\input\\report.pdf');
    expect(native.open).toHaveBeenNthCalledWith(2, { directory: false, multiple: false });

    const handler = vi.fn();
    await api.jobs.onProgress(handler);
    expect(native.listen).toHaveBeenCalledWith(
      'document-studio-job-progress-v1',
      expect.any(Function),
    );
  });

  it('reconciles a sequence gap and ignores stale events', async () => {
    const fetchJob = vi.fn().mockResolvedValue(job);
    const onSnapshot = vi.fn();
    const onEvent = vi.fn();
    const reconcile = createProgressReconciler(fetchJob, onSnapshot, onEvent);

    await reconcile(progress(2));
    await reconcile(progress(2));
    await reconcile(progress(5));

    expect(onEvent).toHaveBeenCalledTimes(2);
    expect(fetchJob).toHaveBeenCalledOnce();
    expect(onSnapshot).toHaveBeenCalledWith(job);
  });

  it('renders only structured safe errors', () => {
    expect(operationErrorMessage({ title: 'Copy failed', detail: 'Try another folder' }))
      .toBe('Copy failed. Try another folder');
    expect(operationErrorMessage(new Error('C:\\secret\\raw-internal.txt')))
      .toBe('Document Studio could not complete that request.');
  });
});
