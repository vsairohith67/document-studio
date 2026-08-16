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
  selectInput: vi.fn(),
  selectDestination: vi.fn(),
  inspect: vi.fn(),
  create: vi.fn(),
  cancel: vi.fn(),
  get: vi.fn(),
  list: vi.fn(),
  scan: vi.fn(),
  status: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    ...actual,
    api: {
      system: { status: mocks.status },
      files: { inspect: mocks.inspect },
      dialogs: {
        selectInput: mocks.selectInput,
        selectDestination: mocks.selectDestination,
      },
      jobs: {
        create: mocks.create,
        cancel: mocks.cancel,
        get: mocks.get,
        onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => {
          mocks.progressHandler = handler;
          return vi.fn();
        }),
      },
      history: { list: mocks.list },
      dependencies: { scan: mocks.scan },
    },
  };
});

import App from './App';

const system: SystemStatus = {
  product: 'Document Studio',
  phase: 'G01 Foundation',
  offlineByDefault: true,
  databaseSchemaVersion: 3,
};

const input: FileInspection = {
  path: 'C:\\input\\report.pdf',
  displayName: 'report.pdf',
  sizeBytes: 4096,
  modifiedAt: '2026-08-16T12:00:00Z',
  mimeType: 'application/pdf',
  fileIdentity: 'volume:file',
};

const dependencies: DependencyDiagnostic[] = [
  {
    id: 'document-studio-core',
    kind: 'built-in',
    status: 'available',
    version: '0.1.0',
    capabilities: ['diagnostic.copy'],
    checkedAt: '2026-08-16T12:00:00Z',
    errorCode: null,
  },
  {
    id: 'qpdf',
    kind: 'deferred',
    status: 'not-required',
    version: null,
    capabilities: [],
    checkedAt: '2026-08-16T12:00:00Z',
    errorCode: null,
  },
];

function makeJob(state: JobRecord['state'] = 'queued'): JobRecord {
  return {
    id: '9e515769-e47b-40dd-a334-ddc661a78d45',
    operationId: 'diagnostic.copy',
    operationVersion: '1.0.0',
    state,
    stage: state === 'completed' ? 'cleanup' : 'inspect',
    sequence: 1,
    progress: { completedUnits: state === 'completed' ? 4096 : 0, totalUnits: 4096, unit: 'bytes' },
    destinationDirectory: 'C:\\output',
    requestedOutputName: 'report-copy.pdf',
    resolvedOutputName: state === 'completed' ? 'report-copy.pdf' : null,
    cancellationRequestedAt: null,
    createdAt: '2026-08-16T12:00:00Z',
    updatedAt: '2026-08-16T12:00:01Z',
    finishedAt: state === 'completed' ? '2026-08-16T12:00:01Z' : null,
    version: 1,
    inputs: [{
      ordinal: 0,
      displayName: 'report.pdf',
      sourcePath: input.path,
      canonicalPath: input.path,
      fileIdentity: input.fileIdentity,
      sizeBytes: input.sizeBytes,
      modifiedAt: input.modifiedAt,
      mimeType: input.mimeType,
      sha256: null,
      passwordReference: null,
    }],
    outputs: [{
      ordinal: 0,
      requestedName: 'report-copy.pdf',
      resolvedName: state === 'completed' ? 'report-copy.pdf' : null,
      stagingPath: null,
      partialPath: null,
      finalPath: state === 'completed' ? 'C:\\output\\report-copy.pdf' : null,
      sizeBytes: state === 'completed' ? 4096 : null,
      mimeType: 'application/octet-stream',
      sha256: null,
      status: state === 'completed' ? 'published' : 'planned',
      verifiedAt: null,
      publishedAt: null,
    }],
    errors: [],
  };
}

function event(state: ProgressEvent['state'], sequence: number): ProgressEvent {
  return {
    schemaVersion: 1,
    sequence,
    emittedAt: '2026-08-16T12:00:01Z',
    jobId: makeJob().id,
    operationId: 'diagnostic.copy',
    state,
    stage: state === 'completed' ? 'cleanup' : 'execute',
    completedUnits: state === 'completed' ? 4096 : 2048,
    totalUnits: 4096,
    unit: 'bytes',
    messageCode: state === 'completed' ? 'COPY_COMPLETED' : 'COPYING_BYTES',
    message: state === 'completed' ? 'Verified copy completed' : 'Copying and checking the file',
    cancellable: state !== 'completed',
  };
}

describe('G01 foundation screen', () => {
  beforeEach(() => {
    mocks.progressHandler = undefined;
    mocks.status.mockResolvedValue(system);
    mocks.scan.mockResolvedValue(dependencies);
    mocks.list.mockResolvedValue([]);
    mocks.selectInput.mockResolvedValue(input.path);
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([input]);
    mocks.create.mockResolvedValue(makeJob());
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
    mocks.get.mockResolvedValue(makeJob('completed'));
  });

  it('uses neutral product copy and has no automated accessibility violations', async () => {
    const { container } = render(<App />);

    expect(await screen.findByText('Offline by default')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Document Studio is ready for local diagnostics' })).toBeTruthy();
    const retiredPersonalName = new RegExp(['Ro', 'hith'].join(''), 'i');
    expect(screen.queryByText(retiredPersonalName)).toBeNull();
    expect((screen.getByRole('button', { name: 'Viewer unavailable in G01' }) as HTMLButtonElement).disabled)
      .toBe(true);
    const accessibility = await axe.run(container, {
      rules: { 'color-contrast': { enabled: false } },
    });
    expect(accessibility.violations).toEqual([]);
  });

  it('runs the keyboard-accessible verified-copy flow and handles cancellation', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText('Offline by default');

    await user.click(screen.getByRole('button', { name: 'Choose file' }));
    await user.click(screen.getByRole('button', { name: 'Choose folder' }));
    const run = screen.getByRole('button', { name: 'Run verified copy' });
    expect((run as HTMLButtonElement).disabled).toBe(false);
    await user.click(run);

    expect(mocks.create).toHaveBeenCalledWith({
      operationId: 'diagnostic.copy',
      inputPaths: [input.path],
      destinationDirectory: 'C:\\output',
      requestedOutputName: 'report-copy.pdf',
    });

    await act(async () => mocks.progressHandler?.(event('running', 2)));
    expect(screen.getByText('Copying and checking the file')).toBeTruthy();
    expect(screen.getByText('50%')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancel).toHaveBeenCalledWith({ jobId: makeJob().id });

    await act(async () => mocks.progressHandler?.(event('completed', 3)));
    expect(await screen.findByText('Verified output: report-copy.pdf')).toBeTruthy();
  });
});
