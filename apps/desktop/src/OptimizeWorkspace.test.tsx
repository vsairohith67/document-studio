import { useState } from 'react';
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  BalancedCompressionVisualSession,
  BalancedCompressionAudit,
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  inspect: vi.fn(), selectPdfInputs: vi.fn(), selectDestination: vi.fn(),
  create: vi.fn(), createBalanced: vi.fn(), balancedAudit: vi.fn(),
  renderBalanced: vi.fn(),
  cancel: vi.fn(), get: vi.fn(), resolveInterrupted: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
  balancedVisualHandler: undefined as ((session: BalancedCompressionVisualSession) => void) | undefined,
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return { ...actual, api: {
    files: {
      inspect: mocks.inspect,
      onPdfDrop: vi.fn(async () => vi.fn()),
    },
    dialogs: {
      selectPdfInputs: mocks.selectPdfInputs,
      selectDestination: mocks.selectDestination,
    },
    jobs: {
      create: mocks.create,
      createBalanced: mocks.createBalanced,
      balancedAudit: mocks.balancedAudit,
      cancel: mocks.cancel,
      get: mocks.get,
      resolveInterrupted: mocks.resolveInterrupted,
      onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => {
        mocks.progressHandler = handler;
        return vi.fn();
      }),
      onBalancedVisualReady: vi.fn(async (handler) => {
        mocks.balancedVisualHandler = handler;
        return vi.fn();
      }),
    },
  } };
});

vi.mock('./viewer/balancedCompression', () => ({
  renderBalancedCompression: mocks.renderBalanced,
}));

import { OptimizeWorkspace } from './OptimizeWorkspace';

const system: SystemStatus = {
  product: 'Document Studio', phase: 'g04a-lossless-pdf-compression',
  offlineByDefault: true, databaseSchemaVersion: 3, webview2RuntimeVersion: '151.0.7922.34',
};
const dependencies: DependencyDiagnostic[] = [{
  id: 'qpdf', kind: 'external', status: 'available', version: '12.3.2',
  capabilities: ['pdf.merge', 'pdf.compress-lossless', 'pdf.compress-balanced'],
  checkedAt: '2026-08-21T12:00:00Z', errorCode: null,
}, {
  id: 'pdfjs', kind: 'built-in', status: 'available', version: '6.2.108',
  capabilities: ['pdf.to-images', 'pdf.compress-balanced'],
  checkedAt: '2026-08-21T12:00:00Z', errorCode: null,
}];
const source: FileInspection = {
  path: 'C:\\input\\signed-report.pdf', displayName: 'signed-report.pdf', sizeBytes: 1000,
  modifiedAt: '2026-08-21T12:00:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file',
};

function job(state: JobRecord['state'], afterBytes: number | null = null): JobRecord {
  return {
    id: '9e515769-e47b-40dd-a334-ddc661a78d45',
    operationId: 'pdf.compress-lossless', operationVersion: '1.0.0', state,
    stage: state === 'completed' ? 'cleanup' : 'inspect', sequence: state === 'completed' ? 3 : 1,
    progress: { completedUnits: state === 'completed' ? 1 : 0, totalUnits: 1, unit: 'items' },
    destinationDirectory: 'C:\\output', requestedOutputName: 'signed-report-compressed.pdf',
    resolvedOutputName: state === 'completed' ? 'signed-report-compressed.pdf' : null,
    cancellationRequestedAt: null, createdAt: '2026-08-21T12:00:00Z', updatedAt: '2026-08-21T12:00:01Z',
    finishedAt: state === 'completed' ? '2026-08-21T12:00:01Z' : null, version: 1,
    completionKind: null, reason: null,
    inputs: [{ ordinal: 0, displayName: source.displayName, sourcePath: source.path, canonicalPath: source.path, fileIdentity: source.fileIdentity, sizeBytes: source.sizeBytes, modifiedAt: source.modifiedAt, mimeType: source.mimeType, sha256: 'a'.repeat(64), passwordReference: null }],
    outputs: [{ ordinal: 0, requestedName: 'signed-report-compressed.pdf', resolvedName: state === 'completed' ? 'signed-report-compressed.pdf' : null, stagingPath: null, partialPath: null, finalPath: state === 'completed' ? 'C:\\output\\signed-report-compressed.pdf' : null, sizeBytes: afterBytes, mimeType: 'application/pdf', sha256: state === 'completed' ? 'b'.repeat(64) : null, status: state === 'completed' ? 'published' : 'planned', verifiedAt: state === 'completed' ? '2026-08-21T12:00:01Z' : null, publishedAt: state === 'completed' ? '2026-08-21T12:00:01Z' : null }],
    errors: [],
  };
}

function noBenefitJob(): JobRecord {
  return {
    ...job('completed'),
    stage: null,
    completionKind: 'no-benefit',
    reason: 'savings-threshold-not-met',
    resolvedOutputName: null,
    outputs: [],
    errors: [],
  };
}

function balancedJob(
  state: JobRecord['state'],
  options: {
    id?: string;
    afterBytes?: number | null;
    completionKind?: JobRecord['completionKind'];
    sequence?: number;
  } = {},
): JobRecord {
  const snapshot = job(state, options.afterBytes ?? (state === 'completed' ? 800 : null));
  return {
    ...snapshot,
    id: options.id ?? snapshot.id,
    operationId: 'pdf.compress-balanced',
    sequence: options.sequence ?? snapshot.sequence,
    completionKind: options.completionKind ?? (state === 'completed' ? 'published' : null),
    reason: options.completionKind === 'no-benefit' ? 'savings-threshold-not-met' : null,
    resolvedOutputName: options.completionKind === 'no-benefit' ? null : snapshot.resolvedOutputName,
    outputs: options.completionKind === 'no-benefit' ? [] : snapshot.outputs,
  };
}

function balancedEvent(
  snapshot: JobRecord,
  sequence = snapshot.sequence,
): ProgressEvent {
  return {
    schemaVersion: 1,
    sequence,
    emittedAt: '2026-08-26T00:00:02Z',
    jobId: snapshot.id,
    operationId: 'pdf.compress-balanced',
    state: snapshot.state,
    stage: snapshot.stage ?? 'cleanup',
    completedUnits: snapshot.progress.completedUnits,
    totalUnits: snapshot.progress.totalUnits,
    unit: snapshot.progress.unit,
    messageCode: snapshot.state === 'completed' ? 'COMPRESSION_COMPLETED' : 'BALANCED_SELECTING_IMAGES',
    message: snapshot.state === 'completed' ? 'Balanced compression completed' : 'Preparing balanced candidate',
    cancellable: snapshot.state !== 'completed' && snapshot.state !== 'publishing',
  };
}

function visualSession(jobId = job('queued').id): BalancedCompressionVisualSession {
  return {
    jobId,
    renderSessionId: 'render-session',
    source: {
      sessionId: 'source-session', generation: 1, sizeBytes: 100,
      fileIdentity: 'source', displayName: 'source.pdf',
      modifiedAt: '2026-08-26T00:00:00Z', mimeType: 'application/pdf',
    },
    candidate: {
      sessionId: 'candidate-session', generation: 1, sizeBytes: 90,
      fileIdentity: 'candidate', displayName: 'candidate.pdf',
      modifiedAt: '2026-08-26T00:00:00Z', mimeType: 'application/pdf',
    },
    pages: [{ pageOrdinal: 0, sourcePageIndex: 0, nonce: 'nonce' }],
    selectedImageCount: 1,
    skippedImageCount: 0,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const auditEvidence: BalancedCompressionAudit = {
  profile: 'balanced-v1', sourceBytes: 1000, candidateBytes: 800,
  savedBytes: 200, savedPercent: 20, selectedImages: 1, skippedImages: 0,
  affectedPages: 1, comparedPages: 1, minimumSsim: 0.999,
  minimumPsnrDb: 48, psnrIsInfinite: false, maximumChangedPixels: 5,
  maximumTotalPixels: 1000, qualityPassed: true, sizeGatePassed: true,
  structuralProofSha256: 'c'.repeat(64), skippedReasons: [],
  createdAt: '2026-08-26T00:00:03Z',
};

function completedEvent(): ProgressEvent {
  return {
    schemaVersion: 1, sequence: 3, emittedAt: '2026-08-21T12:00:02Z',
    jobId: job('queued').id, operationId: 'pdf.compress-lossless', state: 'completed', stage: 'cleanup',
    completedUnits: 1, totalUnits: 1, unit: 'items', messageCode: 'COMPRESSION_COMPLETED',
    message: 'The verified losslessly compressed PDF is ready', cancellable: false,
  };
}

function renderWorkspace() {
  return render(<OptimizeWorkspace system={system} dependencies={dependencies} onOpenMerge={vi.fn()} onOpenViewer={vi.fn()} />);
}

async function prepareBalanced(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Balanced' }));
  await user.click(screen.getByRole('button', { name: /Open PDF/ }));
  await user.click(screen.getByRole('button', { name: 'Choose' }));
}

function NavigationHarness() {
  const [route, setRoute] = useState<'optimize' | 'merge' | 'viewer' | 'convert'>('optimize');
  return route === 'optimize' ? (
    <OptimizeWorkspace
      system={system}
      dependencies={dependencies}
      onOpenMerge={() => setRoute('merge')}
      onOpenViewer={() => setRoute('viewer')}
      onOpenConvert={() => setRoute('convert')}
    />
  ) : <div>{route} route</div>;
}

describe('G04A Optimize workspace', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.progressHandler = undefined;
    mocks.balancedVisualHandler = undefined;
    mocks.selectPdfInputs.mockResolvedValue([source.path]);
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([source]);
    mocks.create.mockResolvedValue(job('queued'));
    mocks.createBalanced.mockResolvedValue({
      ...job('queued'),
      operationId: 'pdf.compress-balanced',
    });
    mocks.get.mockResolvedValue(job('completed', 1250));
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
    mocks.balancedAudit.mockResolvedValue(null);
    mocks.resolveInterrupted.mockResolvedValue(job('failed'));
    mocks.renderBalanced.mockRejectedValue(new Error('The balanced candidate page geometry changed.'));
  });

  it('warns about signatures before execution and has no accessibility violations', async () => {
    const { container } = renderWorkspace();
    expect(screen.getByText(/rewriting invalidates existing digital signatures/i)).toBeTruthy();
    expect(screen.getByText(/smaller, unchanged, or larger/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Balanced' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /aggressive/i })).toBeNull();
    const accessibility = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(accessibility.violations).toEqual([]);
  });

  it('supports the keyboard flow and reports a larger output without claiming savings', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    const open = screen.getByRole('button', { name: /Open PDF/ });
    open.focus();
    await user.keyboard('{Enter}');
    expect(await screen.findByText('Local file preflight ready')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    const compress = screen.getByRole('button', { name: 'Compress Losslessly' });
    compress.focus();
    await user.keyboard(' ');
    expect(mocks.create).toHaveBeenCalledWith({
      operationId: 'pdf.compress-lossless',
      inputPaths: [source.path],
      destinationDirectory: 'C:\\output',
      requestedOutputName: 'signed-report-compressed.pdf',
    });
    await act(async () => { mocks.progressHandler?.(completedEvent()); });
    expect(await screen.findByText('Output grew by 250 B (+25.00%)')).toBeTruthy();
    expect(screen.getByText('+250 B · +25.00%')).toBeTruthy();
    expect(screen.queryByText(/Saved/)).toBeNull();
    expect(document.activeElement).toBe(screen.getByText('Verified compressed PDF').parentElement);
  });

  it('shows the exact no-benefit result and exposes no output action', async () => {
    const user = userEvent.setup();
    mocks.get.mockResolvedValue(noBenefitJob());
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /Open PDF/ }));
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Compress Losslessly' }));
    await act(async () => { mocks.progressHandler?.(completedEvent()); });
    expect(await screen.findByText('No worthwhile size reduction')).toBeTruthy();
    expect(screen.getByText('No output created')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Copy saved path|Open PDF Viewer/i })).toBeNull();
  });

  it('rejects multiple selections and restores focus after removing a source', async () => {
    const user = userEvent.setup();
    mocks.selectPdfInputs.mockResolvedValue([source.path, 'C:\\input\\other.pdf']);
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /Open PDF/ }));
    expect(screen.getByRole('alert').textContent).toContain('exactly one local PDF');

    mocks.selectPdfInputs.mockResolvedValue([source.path]);
    await user.click(screen.getByRole('button', { name: /Open PDF/ }));
    await user.click(await screen.findByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: /Open PDF/ })));
  });

  it('uses only the fixed balanced-v1 profile and explains the refusal boundary', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: 'Balanced' }));
    expect(screen.getByRole('heading', { name: 'Balanced PDF Compression' })).toBeTruthy();
    expect(screen.getByText(/Quality 82, no resampling/i)).toBeTruthy();
    expect(screen.getByText(/Signed documents are refused/i)).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /Open PDF/ }));
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    expect(mocks.createBalanced).toHaveBeenCalledWith({
      operationId: 'pdf.compress-balanced',
      inputPaths: [source.path],
      destinationDirectory: 'C:\\output',
      requestedOutputName: 'signed-report-compressed.pdf',
      settings: { profile: 'balanced-v1' },
    });
  });

  it('cancels and reloads the private job when browser visual verification fails', async () => {
    const user = userEvent.setup();
    mocks.get.mockResolvedValue({
      ...job('failed'),
      operationId: 'pdf.compress-balanced',
      outputs: [],
    });
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: 'Balanced' }));
    await user.click(screen.getByRole('button', { name: /Open PDF/ }));
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => {
      mocks.balancedVisualHandler?.({
        jobId: job('queued').id,
        renderSessionId: 'render-session',
        source: {
          sessionId: 'source-session', generation: 1, sizeBytes: 100,
          fileIdentity: 'source', displayName: 'source.pdf',
          modifiedAt: '2026-08-26T00:00:00Z', mimeType: 'application/pdf',
        },
        candidate: {
          sessionId: 'candidate-session', generation: 1, sizeBytes: 90,
          fileIdentity: 'candidate', displayName: 'candidate.pdf',
          modifiedAt: '2026-08-26T00:00:00Z', mimeType: 'application/pdf',
        },
        pages: [{ pageOrdinal: 0, sourcePageIndex: 0, nonce: 'nonce' }],
        selectedImageCount: 1,
        skippedImageCount: 0,
      });
    });
    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledWith({ jobId: job('queued').id }));
    expect(mocks.get).toHaveBeenCalledWith({ jobId: job('queued').id });
    expect(await screen.findByText('Document Studio could not complete that request.')).toBeTruthy();
  });

  it('reconciles a known pre-visual balanced job exactly once on unmount', async () => {
    const user = userEvent.setup();
    const cancelled = balancedJob('cancelled');
    mocks.get.mockResolvedValue(cancelled);
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));

    view.unmount();

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
    expect(mocks.get).toHaveBeenCalledWith({ jobId: cancelled.id });
    expect(mocks.renderBalanced).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain('C:\\output\\signed-report-compressed.pdf');
  });

  it('preserves dispose intent while native create is held and reconciles the late job', async () => {
    const user = userEvent.setup();
    const heldCreate = deferred<JobRecord>();
    const lateJob = balancedJob('queued', { id: 'late-balanced-job' });
    mocks.createBalanced.mockReturnValue(heldCreate.promise);
    mocks.get.mockResolvedValue(balancedJob('cancelled', { id: lateJob.id }));
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));

    view.unmount();
    heldCreate.resolve(lateJob);

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledWith({ jobId: lateJob.id }));
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    expect(mocks.get).toHaveBeenCalledWith({ jobId: lateJob.id });
    expect(mocks.renderBalanced).not.toHaveBeenCalled();
  });

  it('ignores visual-ready after unmount while the owned native job is reconciled', async () => {
    const user = userEvent.setup();
    mocks.get.mockResolvedValue(balancedJob('cancelled'));
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    const lateVisual = mocks.balancedVisualHandler;

    view.unmount();
    await act(async () => { lateVisual?.(visualSession()); });

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
    expect(mocks.renderBalanced).not.toHaveBeenCalled();
  });

  it('aborts active visual rendering, cancels its RenderTask once, and reconciles native state once', async () => {
    const user = userEvent.setup();
    const heldRender = deferred<JobRecord>();
    const taskCancel = vi.fn();
    let renderSignal: AbortSignal | undefined;
    mocks.get.mockResolvedValue(balancedJob('cancelled'));
    mocks.renderBalanced.mockImplementation(async (_session, signal, hooks) => {
      renderSignal = signal;
      signal.addEventListener('abort', taskCancel, { once: true });
      hooks.onRenderTask({ cancel: taskCancel } as never);
      return heldRender.promise;
    });
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession()); });
    await waitFor(() => expect(mocks.renderBalanced).toHaveBeenCalledTimes(1));

    view.unmount();
    heldRender.reject(new DOMException('Aborted', 'AbortError'));

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
    expect(renderSignal?.aborted).toBe(true);
    expect(taskCancel).toHaveBeenCalledTimes(1);
  });

  it.each([
    ['Merge', 'merge route'],
    ['Viewer', 'viewer route'],
    ['Convert', 'convert route'],
  ])('reconciles a pre-visual job through the enabled %s navigation route', async (button, routeText) => {
    const user = userEvent.setup();
    mocks.get.mockResolvedValue(balancedJob('cancelled'));
    render(<NavigationHarness />);
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));

    await user.click(screen.getByRole('button', { name: button }));

    expect(await screen.findByText(routeText)).toBeTruthy();
    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledTimes(1));
  });

  it('does not cancel completed published or no-benefit jobs during unmount', async () => {
    const user = userEvent.setup();
    const published = balancedJob('completed', { completionKind: 'published' });
    mocks.get.mockResolvedValue(published);
    const publishedView = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.progressHandler?.(balancedEvent(published)); });
    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();
    publishedView.unmount();
    expect(mocks.cancel).not.toHaveBeenCalled();

    vi.clearAllMocks();
    mocks.selectPdfInputs.mockResolvedValue([source.path]);
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([source]);
    const noBenefit = balancedJob('completed', { id: 'no-benefit-job', completionKind: 'no-benefit' });
    mocks.createBalanced.mockResolvedValue(balancedJob('queued', { id: noBenefit.id }));
    mocks.get.mockResolvedValue(noBenefit);
    mocks.balancedAudit.mockResolvedValue(null);
    const noBenefitView = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.progressHandler?.(balancedEvent(noBenefit)); });
    expect(await screen.findByText('No worthwhile size reduction')).toBeTruthy();
    noBenefitView.unmount();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('respects publishing as non-cancellable and never requests deletion on unmount', async () => {
    const user = userEvent.setup();
    const publishing = balancedJob('publishing', { sequence: 2 });
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.progressHandler?.(balancedEvent(publishing)); });

    view.unmount();

    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('can mount again and complete a later operation after prior reconciliation', async () => {
    const user = userEvent.setup();
    const first = balancedJob('queued', { id: 'first-job' });
    mocks.createBalanced.mockResolvedValue(first);
    mocks.get.mockResolvedValue(balancedJob('cancelled', { id: first.id }));
    const firstView = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    firstView.unmount();
    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledWith({ jobId: first.id }));

    const second = balancedJob('queued', { id: 'second-job' });
    const completed = balancedJob('completed', { id: second.id, completionKind: 'published' });
    mocks.createBalanced.mockResolvedValue(second);
    mocks.renderBalanced.mockResolvedValue(completed);
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession(second.id)); });

    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
  });

  it('keeps a published completion authoritative when supplemental audit retrieval fails', async () => {
    const user = userEvent.setup();
    const completed = balancedJob('completed', { completionKind: 'published' });
    mocks.renderBalanced.mockResolvedValue(completed);
    mocks.balancedAudit.mockRejectedValue(new Error('private database detail'));
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession()); });

    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy saved path' })).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByText(/private database detail/i)).toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('keeps a no-benefit completion authoritative when supplemental audit retrieval fails', async () => {
    const user = userEvent.setup();
    const completed = balancedJob('completed', { completionKind: 'no-benefit' });
    mocks.renderBalanced.mockResolvedValue(completed);
    mocks.balancedAudit.mockRejectedValue(new Error('audit unavailable'));
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession()); });

    expect(await screen.findByText('No worthwhile size reduction')).toBeTruthy();
    expect(screen.getByText('No output created')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Copy saved path|Open PDF Viewer/i })).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('renders successful audit evidence without changing the terminal result', async () => {
    const user = userEvent.setup();
    mocks.renderBalanced.mockResolvedValue(balancedJob('completed'));
    mocks.balancedAudit.mockResolvedValue(auditEvidence);
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession()); });

    expect(await screen.findByLabelText('Balanced compression verification evidence')).toBeTruthy();
    expect(screen.getByText(/1 image replaced/i)).toBeTruthy();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('keeps terminal progress reconciliation non-fatal when audit retrieval fails', async () => {
    const user = userEvent.setup();
    const completed = balancedJob('completed');
    mocks.get.mockResolvedValue(completed);
    mocks.balancedAudit.mockRejectedValue(new Error('audit unavailable'));
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.progressHandler?.(balancedEvent(completed)); });

    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('binds delayed audit responses to the exact job generation', async () => {
    const user = userEvent.setup();
    const first = balancedJob('queued', { id: 'audit-job-1' });
    const firstCompleted = balancedJob('completed', { id: first.id });
    const second = balancedJob('queued', { id: 'audit-job-2' });
    const secondCompleted = balancedJob('completed', { id: second.id });
    const oldAudit = deferred<BalancedCompressionAudit | null>();
    const secondAudit = { ...auditEvidence, selectedImages: 2 };
    mocks.createBalanced.mockResolvedValueOnce(first).mockResolvedValueOnce(second);
    mocks.renderBalanced.mockResolvedValueOnce(firstCompleted).mockResolvedValueOnce(secondCompleted);
    mocks.balancedAudit.mockImplementation(({ jobId }: { jobId: string }) => (
      jobId === first.id ? oldAudit.promise : Promise.resolve(secondAudit)
    ));
    renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession(first.id)); });
    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();

    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession(second.id)); });
    expect(await screen.findByText(/2 images replaced/i)).toBeTruthy();
    await act(async () => { oldAudit.resolve(auditEvidence); });

    expect(screen.getByText(/2 images replaced/i)).toBeTruthy();
    expect(screen.queryByText(/1 image replaced/i)).toBeNull();
  });

  it('performs no stale write or cancellation when unmounted during terminal audit read', async () => {
    const user = userEvent.setup();
    const auditRead = deferred<BalancedCompressionAudit | null>();
    mocks.renderBalanced.mockResolvedValue(balancedJob('completed'));
    mocks.balancedAudit.mockReturnValue(auditRead.promise);
    const view = renderWorkspace();
    await prepareBalanced(user);
    await user.click(screen.getByRole('button', { name: 'Compress with Balanced' }));
    await act(async () => { mocks.balancedVisualHandler?.(visualSession()); });
    expect(await screen.findByText('Verified compressed PDF')).toBeTruthy();

    view.unmount();
    await act(async () => { auditRead.resolve(auditEvidence); });

    expect(mocks.cancel).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain('balanced-v1 verification');
  });
});
