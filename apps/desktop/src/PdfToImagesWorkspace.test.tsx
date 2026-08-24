import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  JobRecord,
  PdfToImagesJobSession,
  ViewerDocumentMetadata,
} from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  closeViewer: vi.fn(),
  chooseDestination: vi.fn(),
  revokeDestination: vi.fn(),
  create: vi.fn(),
  cancel: vi.fn(),
  get: vi.fn(),
  onProgress: vi.fn(async () => vi.fn()),
  loadPdfSession: vi.fn(),
  plan: vi.fn(),
  renderJob: vi.fn(),
  closePdf: vi.fn(),
  abortTransport: vi.fn(),
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: () => ({
    getTotalSize: () => 0,
    getVirtualItems: () => [],
    measureElement: vi.fn(),
    scrollToIndex: vi.fn(),
  }),
}));
vi.mock('./viewer/PageSurface', () => ({ PageThumbnail: () => null }));
vi.mock('./viewer/pdfSession', () => ({ loadPdfSession: mocks.loadPdfSession }));
vi.mock('./viewer/pdfToImages', () => ({
  PDF_TO_IMAGES_MAX_OUTPUTS: 128,
  planPdfImagePages: mocks.plan,
  renderPdfImageJob: mocks.renderJob,
}));
vi.mock('./api', () => ({
  api: {
    viewer: {
      open: mocks.open,
      close: mocks.closeViewer,
      chooseDestination: mocks.chooseDestination,
      revokeDestination: mocks.revokeDestination,
    },
    jobs: {
      createPdfToImages: mocks.create,
      cancel: mocks.cancel,
      get: mocks.get,
      onProgress: mocks.onProgress,
    },
  },
  operationErrorMessage: () => 'Document Studio could not complete that request.',
}));

import { PdfToImagesWorkspace } from './PdfToImagesWorkspace';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const metadata: ViewerDocumentMetadata = {
  sessionId: 'opaque-viewer-session',
  generation: 7,
  displayName: 'input.pdf',
  sizeBytes: 512,
  modifiedAt: '2026-08-23T00:00:00Z',
  mimeType: 'application/pdf',
  fileIdentity: 'opaque-file-identity',
};

function job(id: string, state: JobRecord['state']): JobRecord {
  return {
    id,
    operationId: 'pdf.to-images',
    operationVersion: '1.0.0',
    state,
    stage: state === 'queued' ? 'execute' : 'cleanup',
    sequence: state === 'queued' ? 3 : 4,
    progress: { completedUnits: state === 'completed' ? 1 : 0, totalUnits: 1, unit: 'items' },
    destinationDirectory: '',
    requestedOutputName: 'input',
    resolvedOutputName: null,
    cancellationRequestedAt: state === 'cancelled' ? '2026-08-23T00:00:01Z' : null,
    createdAt: '2026-08-23T00:00:00Z',
    updatedAt: '2026-08-23T00:00:01Z',
    finishedAt: state === 'queued' ? null : '2026-08-23T00:00:01Z',
    version: state === 'queued' ? 3 : 4,
    completionKind: null,
    reason: null,
    inputs: [],
    outputs: [{
      ordinal: 0,
      requestedName: 'input-page-0001.png',
      resolvedName: state === 'completed' ? 'input-page-0001.png' : null,
      stagingPath: null,
      partialPath: null,
      finalPath: null,
      sizeBytes: state === 'completed' ? 10 : null,
      mimeType: 'image/png',
      sha256: state === 'completed' ? 'a'.repeat(64) : null,
      status: state === 'completed' ? 'published' : 'planned',
      verifiedAt: state === 'completed' ? '2026-08-23T00:00:01Z' : null,
      publishedAt: state === 'completed' ? '2026-08-23T00:00:01Z' : null,
    }],
    errors: [],
  };
}

function session(id: string): PdfToImagesJobSession {
  return {
    job: job(id, 'queued'),
    renderSessionId: `render-${id}`,
    pages: [{
      pageOrdinal: 0,
      sourcePageIndex: 0,
      nonce: `nonce-${id}`,
      expectedWidth: 1,
      expectedHeight: 1,
    }],
  };
}

async function prepareWorkspace(user: ReturnType<typeof userEvent.setup>) {
  render(<PdfToImagesWorkspace />);
  await user.click(screen.getByRole('button', { name: 'Open PDF' }));
  expect(await screen.findByText('input.pdf')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Choose' }));
  expect(await screen.findByText('Output folder')).toBeTruthy();
}

describe('PDF-to-images operation lifecycle', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.open.mockResolvedValue(metadata);
    mocks.closeViewer.mockResolvedValue(undefined);
    mocks.chooseDestination.mockResolvedValue({ grantId: 'destination-grant', displayName: 'Output folder' });
    mocks.revokeDestination.mockResolvedValue(undefined);
    mocks.onProgress.mockResolvedValue(vi.fn());
    mocks.loadPdfSession.mockImplementation(async (
      _metadata: ViewerDocumentMetadata,
      _onPassword: () => void,
      _onError: (reason: unknown) => void,
      _signal: AbortSignal,
      onResources: (resources: unknown) => void,
    ) => {
      const loaded = {
        document: { numPages: 1 },
        loadingTask: {},
        transport: { abort: mocks.abortTransport },
        close: mocks.closePdf,
      };
      onResources(loaded);
      return loaded;
    });
    mocks.closePdf.mockResolvedValue(undefined);
    mocks.plan.mockResolvedValue([{ sourcePageIndex: 0, width: 1, height: 1 }]);
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
  });

  it('holds retry ownership until a late-created job is terminal, then allows a normal subsequent operation', async () => {
    const user = userEvent.setup();
    const heldCreate = deferred<PdfToImagesJobSession>();
    const heldSourceCleanup = deferred<void>();
    mocks.closePdf.mockReturnValueOnce(heldSourceCleanup.promise);
    mocks.create
      .mockReturnValueOnce(heldCreate.promise)
      .mockResolvedValueOnce(session('next-job'));
    mocks.get.mockImplementation(async ({ jobId }: { jobId: string }) => job(jobId, 'cancelled'));
    mocks.renderJob.mockResolvedValue(job('next-job', 'completed'));
    await prepareWorkspace(user);

    await user.click(screen.getByRole('button', { name: 'Convert pages' }));
    await vi.waitFor(() => expect(mocks.create).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancel).not.toHaveBeenCalled();
    const openWhileHeld = screen.getByRole('button', { name: 'Replace' }) as HTMLButtonElement;
    expect(openWhileHeld.disabled).toBe(true);

    await act(async () => heldCreate.resolve(session('late-job')));
    await vi.waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
    expect(mocks.cancel).toHaveBeenCalledWith({ jobId: 'late-job' });
    expect(mocks.get).toHaveBeenCalledWith({ jobId: 'late-job' });
    expect(await screen.findByText('cancelled')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Replace' }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => heldSourceCleanup.resolve());
    await vi.waitFor(() => expect((screen.getByRole('button', { name: 'Open PDF' }) as HTMLButtonElement).disabled).toBe(false));
    expect(mocks.renderJob).not.toHaveBeenCalled();
    expect(screen.queryByText(/C:\\private/iu)).toBeNull();

    await user.click(screen.getByRole('button', { name: 'Open PDF' }));
    await user.click(screen.getByRole('button', { name: 'Convert pages' }));
    await vi.waitFor(() => expect(mocks.create).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(mocks.renderJob).toHaveBeenCalledTimes(1));
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('completed')).toBeTruthy();
  });

  it('cancels before create starts without creating a native job', async () => {
    const user = userEvent.setup();
    const heldPlan = deferred<Array<{ sourcePageIndex: number; width: number; height: number }>>();
    mocks.plan.mockReturnValueOnce(heldPlan.promise);
    await prepareWorkspace(user);
    await user.click(screen.getByRole('button', { name: 'Convert pages' }));
    await vi.waitFor(() => expect(mocks.plan).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await act(async () => heldPlan.resolve([{ sourcePageIndex: 0, width: 1, height: 1 }]));
    await vi.waitFor(() => expect((screen.getByRole('button', { name: 'Open PDF' }) as HTMLButtonElement).disabled).toBe(false));
    expect(mocks.create).not.toHaveBeenCalled();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('reconciles cancellation during rendering after the job ID is known', async () => {
    const user = userEvent.setup();
    mocks.create.mockResolvedValue(session('rendering-job'));
    mocks.get.mockResolvedValue(job('rendering-job', 'cancelled'));
    mocks.renderJob.mockImplementation(async (
      _pdf: unknown,
      _session: PdfToImagesJobSession,
      _dpi: number,
      signal: AbortSignal,
    ) => new Promise((_resolve, reject) => signal.addEventListener('abort', () => {
      reject(new DOMException('Rendering cancelled', 'AbortError'));
    }, { once: true })));
    await prepareWorkspace(user);
    await user.click(screen.getByRole('button', { name: 'Convert pages' }));
    await vi.waitFor(() => expect(mocks.renderJob).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await vi.waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
    expect(mocks.cancel).toHaveBeenCalledWith({ jobId: 'rendering-job' });
    expect(await screen.findByText('cancelled')).toBeTruthy();
    expect(mocks.get).toHaveBeenCalledTimes(1);
  });

  it('preserves ordinary failure handling without issuing cancellation', async () => {
    const user = userEvent.setup();
    mocks.create.mockResolvedValue(session('failed-job'));
    mocks.renderJob.mockRejectedValue(new Error('ordinary render failure'));
    mocks.get.mockResolvedValue(job('failed-job', 'failed'));
    await prepareWorkspace(user);
    await user.click(screen.getByRole('button', { name: 'Convert pages' }));
    expect((await screen.findByText('ordinary render failure')).textContent).toBe('ordinary render failure');
    expect(mocks.cancel).not.toHaveBeenCalled();
    expect(mocks.get).toHaveBeenCalledWith({ jobId: 'failed-job' });
    expect((screen.getByRole('button', { name: 'Convert pages' }) as HTMLButtonElement).disabled).toBe(false);
  });
});
