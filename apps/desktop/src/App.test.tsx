import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DependencyDiagnostic, FileInspection, JobRecord, ProgressEvent, SystemStatus } from '@document-studio/contracts';

const mocks = vi.hoisted(() => ({
  selectPdfInputs: vi.fn(), selectDestination: vi.fn(), inspect: vi.fn(), create: vi.fn(), cancel: vi.fn(),
  get: vi.fn(), resolveInterrupted: vi.fn(), list: vi.fn(), scan: vi.fn(), status: vi.fn(), settingGet: vi.fn(),
  progressHandler: undefined as ((event: ProgressEvent) => void) | undefined,
  dropHandler: undefined as ((paths: string[]) => void) | undefined,
}));

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return { ...actual, api: {
    system: { status: mocks.status },
    files: {
      inspect: mocks.inspect,
      onPdfDrop: vi.fn(async (handler: (paths: string[]) => void) => { mocks.dropHandler = handler; return vi.fn(); }),
    },
    dialogs: { selectPdfInputs: mocks.selectPdfInputs, selectDestination: mocks.selectDestination },
    jobs: {
      create: mocks.create, cancel: mocks.cancel, get: mocks.get, resolveInterrupted: mocks.resolveInterrupted,
      onProgress: vi.fn(async (handler: (event: ProgressEvent) => void) => { mocks.progressHandler = handler; return vi.fn(); }),
    },
    history: { list: mocks.list }, dependencies: { scan: mocks.scan }, settings: { get: mocks.settingGet },
  } };
});

import App from './App';

const system: SystemStatus = { product: 'Document Studio', phase: 'G02', offlineByDefault: true, databaseSchemaVersion: 3 };
const inputOne: FileInspection = { path: 'C:\\input\\one.pdf', displayName: 'one.pdf', sizeBytes: 4096, modifiedAt: '2026-08-17T12:00:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file-one' };
const inputTwo: FileInspection = { path: 'C:\\input\\two.pdf', displayName: 'two.pdf', sizeBytes: 8192, modifiedAt: '2026-08-17T12:01:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file-two' };
const inputThree: FileInspection = { path: 'C:\\input\\three.pdf', displayName: 'three.pdf', sizeBytes: 12288, modifiedAt: '2026-08-17T12:02:00Z', mimeType: 'application/pdf', fileIdentity: 'volume:file-three' };
const dependencies: DependencyDiagnostic[] = [
  { id: 'document-studio-core', kind: 'built-in', status: 'available', version: '0.1.0', capabilities: ['diagnostic.copy'], checkedAt: '2026-08-17T12:00:00Z', errorCode: null },
  { id: 'qpdf', kind: 'external', status: 'available', version: '12.3.2', capabilities: ['pdf.merge'], checkedAt: '2026-08-17T12:00:00Z', errorCode: null },
];

function makeJob(state: JobRecord['state'] = 'queued'): JobRecord {
  return {
    id: '9e515769-e47b-40dd-a334-ddc661a78d45', operationId: 'pdf.merge', operationVersion: '1.0.0', state,
    stage: state === 'completed' ? 'cleanup' : 'inspect', sequence: 1,
    progress: { completedUnits: state === 'completed' ? 12288 : 0, totalUnits: 12288, unit: 'bytes' },
    destinationDirectory: 'C:\\output', requestedOutputName: 'merged.pdf', resolvedOutputName: state === 'completed' ? 'merged.pdf' : null,
    cancellationRequestedAt: null, createdAt: '2026-08-17T12:00:00Z', updatedAt: '2026-08-17T12:00:01Z', finishedAt: state === 'completed' ? '2026-08-17T12:00:01Z' : null, version: 1,
    inputs: [inputOne, inputTwo].map((input, ordinal) => ({ ordinal, displayName: input.displayName, sourcePath: input.path, canonicalPath: input.path, fileIdentity: input.fileIdentity, sizeBytes: input.sizeBytes, modifiedAt: input.modifiedAt, mimeType: input.mimeType, sha256: null, passwordReference: null })),
    outputs: [{ ordinal: 0, requestedName: 'merged.pdf', resolvedName: state === 'completed' ? 'merged.pdf' : null, stagingPath: null, partialPath: null, finalPath: state === 'completed' ? 'C:\\output\\merged.pdf' : null, sizeBytes: state === 'completed' ? 10000 : null, mimeType: 'application/pdf', sha256: state === 'completed' ? 'a'.repeat(64) : null, status: state === 'completed' ? 'published' : 'planned', verifiedAt: null, publishedAt: null }], errors: [],
  };
}

function progress(messageCode: string, state: ProgressEvent['state'] = 'running'): ProgressEvent {
  return { schemaVersion: 1, sequence: state === 'completed' ? 3 : 2, emittedAt: '2026-08-17T12:00:02Z', jobId: makeJob().id, operationId: 'pdf.merge', state, stage: state === 'completed' ? 'cleanup' : 'execute', completedUnits: 0, totalUnits: 2, unit: 'items', messageCode, message: messageCode === 'MERGING_PDFS' ? 'Merging the ordered PDF snapshots' : 'The verified merged PDF is ready', cancellable: state !== 'completed' };
}

async function renderWithInputs(inspections: FileInspection[]) {
  const user = userEvent.setup();
  mocks.selectPdfInputs.mockResolvedValue(inspections.map((input) => input.path));
  mocks.inspect.mockResolvedValue(inspections);
  render(<App />);
  await screen.findByText('Offline by default');
  await user.click(screen.getByRole('button', { name: /Add PDFs/ }));
  return user;
}

describe('G02 PDF Merge screen', () => {
  beforeEach(() => {
    mocks.progressHandler = undefined; mocks.dropHandler = undefined;
    mocks.status.mockResolvedValue(system); mocks.scan.mockResolvedValue(dependencies); mocks.list.mockResolvedValue([]);
    mocks.selectPdfInputs.mockResolvedValue([inputOne.path, inputTwo.path]); mocks.selectDestination.mockResolvedValue('C:\\output');
    mocks.inspect.mockResolvedValue([inputOne, inputTwo]); mocks.create.mockResolvedValue(makeJob()); mocks.cancel.mockResolvedValue({ outcome: 'requested' });
    mocks.get.mockResolvedValue(makeJob('completed')); mocks.resolveInterrupted.mockResolvedValue(makeJob('failed'));
    mocks.settingGet.mockResolvedValue({ scope: 'application', key: 'history.retention_days', value: 45, version: 1, updatedAt: '2026-08-17T12:00:00Z' });
  });

  it('renders the Precision Paper merge surface and truthful page-only notice accessibly', async () => {
    const { container } = render(<App />);
    expect(await screen.findByText('Offline by default')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Merge PDFs in the order you choose' })).toBeTruthy();
    expect(screen.getByText(/Existing digital signatures will not remain valid/)).toBeTruthy();
    expect(screen.getByText('qpdf 12.3.2 verified')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Viewer unavailable' }) as HTMLButtonElement).disabled).toBe(true);
    const accessibility = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(accessibility.violations).toEqual([]);
  });

  it('keeps the same logical row focused after Alt+ArrowDown', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const movedRow = screen.getByLabelText('1. one.pdf');
    movedRow.focus();
    await user.keyboard('{Alt>}{ArrowDown}{/Alt}');
    expect(screen.getByLabelText('2. one.pdf')).toBe(movedRow);
    expect(document.activeElement).toBe(movedRow);
  });

  it('keeps the same logical row focused after Alt+ArrowUp', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const movedRow = screen.getByLabelText('2. two.pdf');
    movedRow.focus();
    await user.keyboard('{Alt>}{ArrowUp}{/Alt}');
    expect(screen.getByLabelText('1. two.pdf')).toBe(movedRow);
    expect(document.activeElement).toBe(movedRow);
  });

  it('keeps the same Move down control focused after button reorder', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const moveDown = screen.getByRole('button', { name: 'Move one.pdf down' });
    await user.click(moveDown);
    expect(screen.getByLabelText('2. one.pdf').contains(moveDown)).toBe(true);
    expect(document.activeElement).toBe(moveDown);
  });

  it('keeps the same Move up control focused after button reorder', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const moveUp = screen.getByRole('button', { name: 'Move three.pdf up' });
    await user.click(moveUp);
    expect(screen.getByLabelText('2. three.pdf').contains(moveUp)).toBe(true);
    expect(document.activeElement).toBe(moveUp);
  });

  it('focuses the next row after deleting the first row', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const nextRow = screen.getByLabelText('2. two.pdf');
    screen.getByLabelText('1. one.pdf').focus();
    await user.keyboard('{Delete}');
    expect(screen.getByLabelText('1. two.pdf')).toBe(nextRow);
    expect(document.activeElement).toBe(nextRow);
  });

  it('focuses the next row after deleting a middle row', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const nextRow = screen.getByLabelText('3. three.pdf');
    screen.getByLabelText('2. two.pdf').focus();
    await user.keyboard('{Delete}');
    expect(screen.getByLabelText('2. three.pdf')).toBe(nextRow);
    expect(document.activeElement).toBe(nextRow);
  });

  it('focuses the previous row after deleting the last row', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const previousRow = screen.getByLabelText('2. two.pdf');
    screen.getByLabelText('3. three.pdf').focus();
    await user.keyboard('{Delete}');
    expect(document.activeElement).toBe(previousRow);
  });

  it('focuses the row taking the removed position after using Remove', async () => {
    const user = await renderWithInputs([inputOne, inputTwo, inputThree]);
    const nextRow = screen.getByLabelText('2. two.pdf');
    await user.click(screen.getByRole('button', { name: 'Remove one.pdf' }));
    expect(screen.getByLabelText('1. two.pdf')).toBe(nextRow);
    expect(document.activeElement).toBe(nextRow);
  });

  it('distinguishes duplicate-path rows by stable selection ID while restoring focus', async () => {
    const user = await renderWithInputs([inputOne, inputOne]);
    const duplicateRows = screen.getAllByRole('listitem');
    expect(duplicateRows[0]?.dataset.selectionId).not.toBe(duplicateRows[1]?.dataset.selectionId);
    const remainingDuplicate = duplicateRows[1];
    duplicateRows[0]?.focus();
    await user.keyboard('{Delete}');
    expect(screen.getByLabelText('1. one.pdf')).toBe(remainingDuplicate);
    expect(document.activeElement).toBe(remainingDuplicate);
  });

  it('returns focus to Add PDFs after removing the final row', async () => {
    const user = await renderWithInputs([inputOne]);
    screen.getByLabelText('1. one.pdf').focus();
    await user.keyboard('{Delete}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /Add PDFs/ }));
  });

  it('passes the exact displayed order to pdf.merge and supports native drop', async () => {
    const user = userEvent.setup(); render(<App />); await screen.findByText('Offline by default');
    await act(async () => mocks.dropHandler?.([inputOne.path, inputTwo.path]));
    await user.click(screen.getByRole('button', { name: 'Move two.pdf up' }));
    await user.click(screen.getByRole('button', { name: 'Choose' }));
    await user.click(screen.getByRole('button', { name: 'Merge PDFs' }));
    expect(mocks.create).toHaveBeenCalledWith({ operationId: 'pdf.merge', inputPaths: [inputTwo.path, inputOne.path], destinationDirectory: 'C:\\output', requestedOutputName: 'merged.pdf' });
  });

  it('shows indeterminate qpdf work, cancellation and verified success without fake percentages', async () => {
    const user = userEvent.setup(); render(<App />); await screen.findByText('Offline by default');
    await user.click(screen.getByRole('button', { name: /Add PDFs/ })); await user.click(screen.getByRole('button', { name: 'Choose' })); await user.click(screen.getByRole('button', { name: 'Merge PDFs' }));
    await act(async () => mocks.progressHandler?.(progress('MERGING_PDFS')));
    expect(screen.getByRole('progressbar', { name: 'Merging PDFs' })).toBeTruthy();
    expect(screen.queryByText('0%')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancel).toHaveBeenCalledWith({ jobId: makeJob().id });
    await act(async () => mocks.progressHandler?.(progress('MERGE_COMPLETED', 'completed')));
    expect(await screen.findByText('Verified merged PDF')).toBeTruthy();
    expect(screen.getByText('C:\\output\\merged.pdf')).toBeTruthy();
  });

  it('keeps interrupted recovery evidence visible in metadata-only history', async () => {
    const user = userEvent.setup(); const interrupted = makeJob('interrupted'); mocks.list.mockResolvedValue([interrupted]);
    render(<App />); await user.click(await screen.findByRole('button', { name: 'Resolve safely' }));
    expect(mocks.resolveInterrupted).toHaveBeenCalledWith({ jobId: interrupted.id });
  });

  it('supports pointer drag reorder without making it the only reorder path', async () => {
    const user = userEvent.setup(); render(<App />); await screen.findByText('Offline by default'); await user.click(screen.getByRole('button', { name: /Add PDFs/ }));
    const first = screen.getByLabelText('1. one.pdf'); const second = screen.getByLabelText('2. two.pdf');
    fireEvent.dragStart(first); fireEvent.dragOver(second); fireEvent.drop(second);
    expect(screen.getAllByRole('listitem')[0]?.getAttribute('aria-label')).toBe('1. two.pdf');
  });
});
