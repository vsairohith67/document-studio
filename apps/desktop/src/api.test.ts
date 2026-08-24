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

  it('normalizes Tauri raw range responses without accepting base64 or objects', async () => {
    native.invoke.mockResolvedValueOnce([0x25, 0x50, 0x44, 0x46]);
    await expect(api.viewer.readRange({
      sessionId: 'opaque-session', generation: 4, begin: 0, end: 4,
    })).resolves.toEqual(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
    expect(native.invoke).toHaveBeenLastCalledWith('viewer_read_range', {
      request: { sessionId: 'opaque-session', generation: 4, begin: 0, end: 4 },
    });

    native.invoke.mockResolvedValueOnce('JVBERg==');
    await expect(api.viewer.readRange({
      sessionId: 'opaque-session', generation: 4, begin: 0, end: 4,
    })).rejects.toThrow('not raw byte data');
  });

  it('uploads RGBA only as a raw body with authenticated identity headers', async () => {
    const pixels = new Uint8Array([255, 255, 255, 255]);
    const session = {
      job: { ...job, id: '9e515769-e47b-40dd-a334-ddc661a78d45' },
      renderSessionId: 'c118bb0d-bada-4f44-b5ef-4fcf12bb7512',
      pages: [{
        pageOrdinal: 0,
        sourcePageIndex: 2,
        nonce: '9503134c-4071-4485-a753-e956dd8858e6',
        expectedWidth: 1,
        expectedHeight: 1,
      }],
    };
    await api.jobs.submitPdfPixels(session as never, session.pages[0], pixels);
    expect(native.invoke).toHaveBeenLastCalledWith(
      'pdf_to_images_submit_page',
      pixels,
      { headers: {
        'x-document-studio-job-id': session.job.id,
        'x-document-studio-render-session-id': session.renderSessionId,
        'x-document-studio-page-ordinal': '0',
        'x-document-studio-page-nonce': session.pages[0].nonce,
        'x-document-studio-expected-width': '1',
        'x-document-studio-expected-height': '1',
      } },
    );
    expect(JSON.stringify(native.invoke.mock.calls.at(-1))).not.toContain('destination');
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
