import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { BatchPreviewResponse, BatchRecord, FileInspection, SystemStatus } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  inspect: vi.fn(), selectPdfInputs: vi.fn(), selectDestination: vi.fn(),
  preview: vi.fn(), create: vi.fn(), get: vi.fn(), jobsCreate: vi.fn(),
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return { ...actual, api: {
    files: { inspect: mocks.inspect },
    dialogs: { selectPdfInputs: mocks.selectPdfInputs, selectDestination: mocks.selectDestination },
    batches: { preview: mocks.preview, create: mocks.create, get: mocks.get },
    jobs: { create: mocks.jobsCreate },
  } };
});

import { BatchWorkspace } from './BatchWorkspace';

const system: SystemStatus = {
  product: 'Document Studio', phase: 'g04f1-batch-preview', offlineByDefault: true,
  databaseSchemaVersion: 8, webview2RuntimeVersion: '151.0.7922.34',
};
const files: FileInspection[] = [
  { path: 'C:\\input\\alpha.pdf', displayName: 'alpha.pdf', sizeBytes: 1024, modifiedAt: '2026-08-26T08:00:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file-1' },
  { path: 'C:\\input\\beta.pdf', displayName: 'beta.pdf', sizeBytes: 2048, modifiedAt: '2026-08-26T08:01:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file-2' },
];
const preview: BatchPreviewResponse = {
  schemaVersion: 1, operationId: 'pdf.compress-lossless', operationVersion: '1.0.0',
  settingsSha256: 'a'.repeat(64), namingTemplate: '{stem}-compressed.pdf',
  rows: [
    { ordinal: 0, sourceName: 'alpha.pdf', outputName: 'alpha-compressed.pdf', collisionIndex: 0, sizeBytes: 1024 },
    { ordinal: 1, sourceName: 'beta.pdf', outputName: 'beta-compressed (1).pdf', collisionIndex: 1, sizeBytes: 2048 },
  ],
  diskEstimate: { workspacePeakBytes: 1024, destinationTotalBytes: 3072, combinedRequiredBytes: 4096, workspaceAndDestinationShareVolume: true },
  previewSha256: 'b'.repeat(64), canonicalSizeBytes: 1200, optimisticVersion: 0,
};
const batch: BatchRecord = {
  id: '018f0f17-2f4a-7fb1-a247-303030303030', schemaVersion: 1,
  operationId: 'pdf.compress-lossless', operationVersion: '1.0.0', state: 'queued',
  previewSha256: preview.previewSha256, settingsSha256: preview.settingsSha256,
  namingTemplate: preview.namingTemplate, optimisticVersion: preview.optimisticVersion,
  diskEstimate: preview.diskEstimate,
  progress: { settledChildren: 0, totalChildren: 2, completedChildren: 0, failedChildren: 0, cancelledChildren: 0, interruptedChildren: 0, publishedChildren: 0, noBenefitChildren: 0 },
  createdAt: '2026-08-26T08:02:00Z', updatedAt: '2026-08-26T08:02:00Z', version: 0,
  children: preview.rows.map((item, ordinal) => ({
    ordinal, jobId: `018f0f17-2f4a-7fb1-a247-30303030303${ordinal + 1}`, state: 'queued', completionKind: null, reason: null,
    requestedName: item.outputName.replace(' (1)', ''), plannedName: item.outputName, collisionIndex: item.collisionIndex,
    progress: { completedUnits: 0, totalUnits: files[ordinal].sizeBytes, unit: 'bytes' },
  })),
};

function renderWorkspace() {
  return render(<BatchWorkspace system={system} onOpenMerge={vi.fn()} onOpenViewer={vi.fn()} onOpenOptimize={vi.fn()} onOpenConvert={vi.fn()} />);
}

describe('G04F1 batch preview workspace', () => {
  beforeEach(() => {
    mocks.selectPdfInputs.mockResolvedValue(files.map((file) => file.path));
    mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue(files);
    mocks.preview.mockResolvedValue(preview);
    mocks.create.mockResolvedValue(batch);
  });

  it('creates only the exact eligible preview and remains accessible', async () => {
    const user = userEvent.setup();
    const { container } = renderWorkspace();
    await user.click(screen.getByRole('button', { name: /add pdfs/i }));
    await user.click(screen.getByRole('button', { name: /choose/i }));
    await user.click(screen.getByRole('button', { name: /create preview/i }));

    await waitFor(() => expect(mocks.preview).toHaveBeenCalledWith({
      schemaVersion: 1,
      operationId: 'pdf.compress-lossless',
      operationVersion: '1.0.0',
      settings: {},
      inputPaths: files.map((file) => file.path),
      destinationDirectory: 'C:\\output',
      namingTemplate: '{stem}-compressed.pdf',
    }));
    expect(screen.getByText('beta-compressed (1).pdf')).toBeTruthy();
    expect(screen.getByText(/does not start a worker/i)).toBeTruthy();
    expect(
      screen.getByRole('heading', { name: 'Canonical preview' }).closest('article')?.hasAttribute('aria-live'),
    ).toBe(false);
    expect((await axe.run(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
  });

  it('creates atomic metadata without invoking the ordinary worker command', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /add pdfs/i }));
    await user.click(screen.getByRole('button', { name: /choose/i }));
    await user.click(screen.getByRole('button', { name: /create preview/i }));
    await user.click(await screen.findByRole('button', { name: /create batch metadata/i }));

    await waitFor(() => expect(mocks.create).toHaveBeenCalledWith(expect.objectContaining({
      operationId: 'pdf.compress-lossless', operationVersion: '1.0.0', previewSha256: preview.previewSha256,
      optimisticVersion: preview.optimisticVersion,
    })));
    expect(mocks.jobsCreate).not.toHaveBeenCalled();
    expect(screen.getAllByText(/no child was started/i)).toHaveLength(2);
    expect(screen.getByText('0 of 2 children settled')).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole('status', { name: /batch creation confirmation/i }));
  });

  it('invalidates a preview when the naming template changes', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /add pdfs/i }));
    await user.click(screen.getByRole('button', { name: /choose/i }));
    await user.click(screen.getByRole('button', { name: /create preview/i }));
    const template = await screen.findByRole('textbox', { name: /naming template/i });
    await user.clear(template);
    await user.type(template, '{stem}-{index}.pdf');
    expect(screen.queryByRole('button', { name: /create batch metadata/i })).toBeNull();
  });

  it('restores focus deterministically after an ordered input is removed', async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole('button', { name: /add pdfs/i }));
    await user.click(screen.getByRole('button', { name: /remove alpha.pdf/i }));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /remove beta.pdf/i }));
    await user.click(screen.getByRole('button', { name: /remove beta.pdf/i }));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /add pdfs/i }));
  });
});
