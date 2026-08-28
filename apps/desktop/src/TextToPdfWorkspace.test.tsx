import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { JobRecord, ProgressEvent, TextInputMetadata } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  openOutput: vi.fn(),
  chooseDestination: vi.fn(),
  close: vi.fn(),
  revokeDestination: vi.fn(),
  createTextToPdf: vi.fn(),
  cancel: vi.fn(),
  get: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return { ...actual, api: {
    text: { open: mocks.open, openOutput: mocks.openOutput },
    viewer: {
      chooseDestination: mocks.chooseDestination,
      close: mocks.close,
      revokeDestination: mocks.revokeDestination,
    },
    jobs: {
      createTextToPdf: mocks.createTextToPdf,
      cancel: mocks.cancel,
      get: mocks.get,
      onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => {
        mocks.progressHandler = handler;
        return vi.fn();
      }),
    },
  } };
});

import { TextToPdfWorkspace } from './TextToPdfWorkspace';

const input: TextInputMetadata = {
  sessionId: '400743e6-1b21-4a5c-8e7c-8d489f65c307',
  generation: 7,
  displayName: 'private-notes.txt',
  sizeBytes: 321,
  modifiedAt: '2026-08-28T00:00:00Z',
  mimeType: 'text/plain',
};

function job(state: JobRecord['state']): JobRecord {
  const completed = state === 'completed';
  return {
    id: '9e515769-e47b-40dd-a334-ddc661a78d46', operationId: 'text.to-pdf', operationVersion: '1.0.0', state,
    stage: completed ? 'cleanup' : 'inspect', sequence: completed ? 18 : 1,
    progress: { completedUnits: completed ? 18 : 0, totalUnits: 18, unit: 'steps' },
    destinationDirectory: '', requestedOutputName: 'private-notes.pdf', resolvedOutputName: completed ? 'private-notes.pdf' : null,
    cancellationRequestedAt: null, createdAt: '2026-08-28T00:00:00Z', updatedAt: '2026-08-28T00:00:01Z', finishedAt: completed ? '2026-08-28T00:00:01Z' : null, version: completed ? 18 : 1,
    completionKind: completed ? 'published' : null, reason: null,
    inputs: [{ ordinal: 0, displayName: 'Selected TXT', sourcePath: '', canonicalPath: '', fileIdentity: 'volume:file', sizeBytes: 321, modifiedAt: input.modifiedAt, mimeType: 'text/plain', sha256: completed ? 'a'.repeat(64) : null, passwordReference: null }],
    outputs: [{ ordinal: 0, requestedName: 'private-notes.pdf', resolvedName: completed ? 'private-notes.pdf' : null, stagingPath: null, partialPath: null, finalPath: completed ? 'C:\\output\\private-notes.pdf' : null, sizeBytes: completed ? 4567 : null, mimeType: 'application/pdf', sha256: completed ? 'b'.repeat(64) : null, status: completed ? 'published' : 'planned', verifiedAt: completed ? '2026-08-28T00:00:01Z' : null, publishedAt: completed ? '2026-08-28T00:00:01Z' : null }],
    errors: [],
  };
}

function completedEvent(): ProgressEvent {
  return { schemaVersion: 1, sequence: 18, emittedAt: '2026-08-28T00:00:02Z', jobId: job('queued').id, operationId: 'text.to-pdf', state: 'completed', stage: 'cleanup', completedUnits: 18, totalUnits: 18, unit: 'steps', messageCode: 'TXT_COMPLETED', message: 'The verified TXT PDF is ready', cancellable: false };
}

describe('G04E1 TXT to PDF workspace', () => {
  beforeEach(() => {
    mocks.progressHandler = undefined;
    mocks.open.mockResolvedValue(input);
    mocks.openOutput.mockResolvedValue({
      sessionId: '9b8e45bc-6fc1-4c58-93e6-507d91c53ca5',
      generation: 8,
      displayName: 'private-notes.pdf',
      sizeBytes: 4567,
      modifiedAt: '2026-08-28T00:00:01Z',
      mimeType: 'application/pdf',
      fileIdentity: 'volume:published',
    });
    mocks.chooseDestination.mockResolvedValue({ grantId: '14035919-f030-4a43-b6a6-60feccf42557', displayName: 'Output folder' });
    mocks.close.mockResolvedValue(undefined);
    mocks.revokeDestination.mockResolvedValue(undefined);
    mocks.createTextToPdf.mockResolvedValue(job('queued'));
    mocks.cancel.mockResolvedValue({ outcome: 'requested' });
    mocks.get.mockResolvedValue(job('completed'));
  });

  it('uses opaque input/destination identities and the exact frozen settings', async () => {
    const user = userEvent.setup();
    const onBusyChange = vi.fn();
    const onOpenViewer = vi.fn();
    render(<TextToPdfWorkspace onBusyChange={onBusyChange} onOpenViewer={onOpenViewer} />);
    await user.click(screen.getByRole('button', { name: 'Choose TXT' }));
    await user.click(screen.getByRole('radio', { name: 'Letter' }));
    await user.click(screen.getByRole('radio', { name: 'Landscape' }));
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Create verified PDF' }));
    expect(mocks.createTextToPdf).toHaveBeenCalledWith({
      operationId: 'text.to-pdf',
      inputSessionId: input.sessionId,
      inputGeneration: input.generation,
      destinationGrantId: '14035919-f030-4a43-b6a6-60feccf42557',
      requestedOutputName: 'private-notes.pdf',
      settings: { pageSize: 'letter', orientation: 'landscape' },
    });
    expect(JSON.stringify(mocks.createTextToPdf.mock.calls)).not.toContain('C:\\input');
    await act(async () => { mocks.progressHandler?.(completedEvent()); });
    expect(await screen.findByText('Verified TXT PDF')).toBeTruthy();
    const savedPath = screen.getByText('C:\\output\\private-notes.pdf');
    expect(savedPath).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Open in Viewer' }));
    expect(mocks.openOutput).toHaveBeenCalledWith(job('completed').id);
    expect(onOpenViewer).toHaveBeenCalledWith(expect.objectContaining({ displayName: 'private-notes.pdf' }));
    await user.click(screen.getByRole('button', { name: 'Reveal saved location' }));
    expect(document.activeElement).toBe(savedPath);
  });

  it('has bounded explanations, no document preview, and no accessibility violations', async () => {
    const user = userEvent.setup();
    const { container } = render(<TextToPdfWorkspace onBusyChange={vi.fn()} onOpenViewer={vi.fn()} />);
    await user.click(screen.getByRole('button', { name: 'Choose TXT' }));
    expect(screen.getByText(/Strict UTF-8 only/)).toBeTruthy();
    expect(screen.getByText(/English, Hindi \(Devanagari\), and Telugu/)).toBeTruthy();
    expect(screen.queryByRole('textbox', { name: /preview/i })).toBeNull();
    const accessibility = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(accessibility.violations).toEqual([]);
  });
});
