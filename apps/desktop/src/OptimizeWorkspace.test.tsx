import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  inspect: vi.fn(), selectPdfInputs: vi.fn(), selectDestination: vi.fn(),
  create: vi.fn(), cancel: vi.fn(), get: vi.fn(), resolveInterrupted: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
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
      cancel: mocks.cancel,
      get: mocks.get,
      resolveInterrupted: mocks.resolveInterrupted,
      onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => {
        mocks.progressHandler = handler;
        return vi.fn();
      }),
    },
  } };
});

import { OptimizeWorkspace } from './OptimizeWorkspace';

const system: SystemStatus = {
  product: 'Document Studio', phase: 'g04a-lossless-pdf-compression',
  offlineByDefault: true, databaseSchemaVersion: 3, webview2RuntimeVersion: '151.0.7922.34',
};
const dependencies: DependencyDiagnostic[] = [{
  id: 'qpdf', kind: 'external', status: 'available', version: '12.3.2',
  capabilities: ['pdf.merge', 'pdf.compress-lossless'],
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
    inputs: [{ ordinal: 0, displayName: source.displayName, sourcePath: source.path, canonicalPath: source.path, fileIdentity: source.fileIdentity, sizeBytes: source.sizeBytes, modifiedAt: source.modifiedAt, mimeType: source.mimeType, sha256: 'a'.repeat(64), passwordReference: null }],
    outputs: [{ ordinal: 0, requestedName: 'signed-report-compressed.pdf', resolvedName: state === 'completed' ? 'signed-report-compressed.pdf' : null, stagingPath: null, partialPath: null, finalPath: state === 'completed' ? 'C:\\output\\signed-report-compressed.pdf' : null, sizeBytes: afterBytes, mimeType: 'application/pdf', sha256: state === 'completed' ? 'b'.repeat(64) : null, status: state === 'completed' ? 'published' : 'planned', verifiedAt: state === 'completed' ? '2026-08-21T12:00:01Z' : null, publishedAt: state === 'completed' ? '2026-08-21T12:00:01Z' : null }],
    errors: [],
  };
}

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

describe('G04A Optimize workspace', () => {
  beforeEach(() => {
    mocks.progressHandler = undefined;
    mocks.selectPdfInputs.mockResolvedValue([source.path]);
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([source]);
    mocks.create.mockResolvedValue(job('queued'));
    mocks.get.mockResolvedValue(job('completed', 1250));
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
    mocks.resolveInterrupted.mockResolvedValue(job('failed'));
  });

  it('warns about signatures before execution and has no accessibility violations', async () => {
    const { container } = renderWorkspace();
    expect(screen.getByText(/rewriting invalidates existing digital signatures/i)).toBeTruthy();
    expect(screen.getByText(/smaller, unchanged, or larger/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /balanced|aggressive/i })).toBeNull();
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
    const compress = screen.getByRole('button', { name: 'Compress' });
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
});
