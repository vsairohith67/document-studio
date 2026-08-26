import { act, render, screen } from '@testing-library/react';
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
  inspect: vi.fn(), selectImageInputs: vi.fn(), selectDestination: vi.fn(),
  create: vi.fn(), cancel: vi.fn(), get: vi.fn(), warnings: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return { ...actual, api: {
    files: { inspect: mocks.inspect, onPdfDrop: vi.fn(async () => vi.fn()) },
    dialogs: {
      selectImageInputs: mocks.selectImageInputs,
      selectDestination: mocks.selectDestination,
    },
    jobs: {
      create: mocks.create,
      cancel: mocks.cancel,
      get: mocks.get,
      warnings: mocks.warnings,
      onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => {
        mocks.progressHandler = handler;
        return vi.fn();
      }),
    },
  } };
});

import { ConvertWorkspace } from './ConvertWorkspace';

const system: SystemStatus = {
  product: 'Document Studio', phase: 'g04b2-pdf-to-images',
  offlineByDefault: true, databaseSchemaVersion: 6, webview2RuntimeVersion: '151.0.7922.34',
};
const dependencies: DependencyDiagnostic[] = [
  { id: 'document-studio-core', kind: 'built-in', status: 'available', version: '0.1.0', capabilities: ['image.to-pdf'], checkedAt: '2026-08-22T00:00:00Z', errorCode: null },
  { id: 'qpdf', kind: 'external', status: 'available', version: '12.3.2', capabilities: ['image.to-pdf'], checkedAt: '2026-08-22T00:00:00Z', errorCode: null },
];
const first: FileInspection = { path: 'C:\\input\\first.png', displayName: 'first.png', sizeBytes: 100, modifiedAt: '2026-08-22T00:00:00Z', mimeType: 'image/png', fileIdentity: 'volume:first' };
const second: FileInspection = { path: 'C:\\input\\second.webp', displayName: 'second.webp', sizeBytes: 200, modifiedAt: '2026-08-22T00:00:00Z', mimeType: 'image/webp', fileIdentity: 'volume:second' };

function job(state: JobRecord['state']): JobRecord {
  return {
    id: '9e515769-e47b-40dd-a334-ddc661a78d46', operationId: 'image.to-pdf', operationVersion: '1.0.0', state,
    stage: state === 'completed' ? 'cleanup' : 'inspect', sequence: state === 'completed' ? 4 : 1,
    progress: { completedUnits: state === 'completed' ? 2 : 0, totalUnits: 2, unit: 'items' },
    destinationDirectory: 'C:\\output', requestedOutputName: 'images.pdf', resolvedOutputName: state === 'completed' ? 'images.pdf' : null,
    cancellationRequestedAt: null, createdAt: '2026-08-22T00:00:00Z', updatedAt: '2026-08-22T00:00:01Z', finishedAt: state === 'completed' ? '2026-08-22T00:00:01Z' : null, version: 1,
    completionKind: null, reason: null,
    inputs: [first, second].map((image, ordinal) => ({ ordinal, displayName: image.displayName, sourcePath: image.path, canonicalPath: image.path, fileIdentity: image.fileIdentity, sizeBytes: image.sizeBytes, modifiedAt: image.modifiedAt, mimeType: image.mimeType, sha256: state === 'completed' ? 'a'.repeat(64) : null, passwordReference: null })),
    outputs: [{ ordinal: 0, requestedName: 'images.pdf', resolvedName: state === 'completed' ? 'images.pdf' : null, stagingPath: null, partialPath: null, finalPath: state === 'completed' ? 'C:\\output\\images.pdf' : null, sizeBytes: state === 'completed' ? 300 : null, mimeType: 'application/pdf', sha256: state === 'completed' ? 'b'.repeat(64) : null, status: state === 'completed' ? 'published' : 'planned', verifiedAt: state === 'completed' ? '2026-08-22T00:00:01Z' : null, publishedAt: state === 'completed' ? '2026-08-22T00:00:01Z' : null }],
    errors: [],
  };
}

function completedEvent(): ProgressEvent {
  return { schemaVersion: 1, sequence: 4, emittedAt: '2026-08-22T00:00:02Z', jobId: job('queued').id, operationId: 'image.to-pdf', state: 'completed', stage: 'cleanup', completedUnits: 2, totalUnits: 2, unit: 'items', messageCode: 'IMAGE_PDF_COMPLETED', message: 'The verified image PDF is ready', cancellable: false };
}

function renderWorkspace() {
  return render(<ConvertWorkspace system={system} dependencies={dependencies} onOpenMerge={vi.fn()} onOpenViewer={vi.fn()} onOpenOptimize={vi.fn()} />);
}

describe('G04B Convert workspace', () => {
  beforeEach(() => {
    mocks.progressHandler = undefined;
    mocks.selectImageInputs.mockResolvedValue([first.path, second.path]);
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([first, second]);
    mocks.create.mockResolvedValue(job('queued'));
    mocks.get.mockResolvedValue(job('completed'));
    mocks.warnings.mockResolvedValue([{ code: 'ICC_PROFILE_NOT_RETAINED', sanitizedDetail: 'The embedded color profile was not retained; decoded pixel values use DeviceRGB.', inputIndex: 1, pageIndex: 1, createdAt: '2026-08-22T00:00:01Z' }]);
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
  });

  it('enables the accepted PDF.js renderer direction and has no accessibility violations', async () => {
    const user = userEvent.setup();
    const { container } = renderWorkspace();
    expect(screen.getByRole('tab', { name: /Images to PDF/ }).getAttribute('aria-selected')).toBe('true');
    expect((screen.getByRole('tab', { name: /PDF to images/ }) as HTMLButtonElement).disabled).toBe(false);
    await user.click(screen.getByRole('tab', { name: /PDF to images/ }));
    expect(screen.getByRole('heading', { name: 'Select pages and output order' })).toBeTruthy();
    expect(screen.getByText('PDF.js 6.2.108')).toBeTruthy();
    const accessibility = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(accessibility.violations).toEqual([]);
  });

  it('exposes the enabled Batch navigation route', async () => {
    const user = userEvent.setup();
    const onOpenBatch = vi.fn();
    render(
      <ConvertWorkspace
        system={system}
        dependencies={dependencies}
        onOpenMerge={vi.fn()}
        onOpenViewer={vi.fn()}
        onOpenOptimize={vi.fn()}
        onOpenBatch={onOpenBatch}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Batch' }));
    expect(onOpenBatch).toHaveBeenCalledTimes(1);
  });

  it('preserves the displayed order, supports keyboard reordering, and reports verified completion', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /Add images/ }));
    const rows = await screen.findAllByRole('listitem');
    rows[1].focus();
    await user.keyboard('{Alt>}{ArrowUp}{/Alt}');
    expect(screen.getAllByRole('listitem')[0].textContent).toContain('second.webp');
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Create PDF' }));
    expect(mocks.create).toHaveBeenCalledWith({
      operationId: 'image.to-pdf',
      inputPaths: [second.path, first.path],
      destinationDirectory: 'C:\\output',
      requestedOutputName: 'images.pdf',
    });
    await act(async () => { mocks.progressHandler?.(completedEvent()); });
    expect(await screen.findByText('Verified image PDF')).toBeTruthy();
    expect(screen.getByText('C:\\output\\images.pdf')).toBeTruthy();
    expect(screen.getByText(/embedded color profile was not retained/)).toBeTruthy();
  });
});
