import Ajv2020 from 'ajv/dist/2020';
import addFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';
import fixtures from '../fixtures/foundation-contracts.json';
import pdfMergeFixtures from '../fixtures/pdf-merge-contracts.json';
import pdfCompressLosslessFixtures from '../fixtures/pdf-compress-lossless-contracts.json';
import pdfCompressBalancedFixtures from '../fixtures/pdf-compress-balanced-contracts.json';
import pdfToImagesFixtures from '../fixtures/pdf-to-images-contracts.json';
import textToPdfFixtures from '../fixtures/text-to-pdf-contracts.json';
import batchFixtures from '../fixtures/batch-preview-contracts.json';
import batchSchema from '../batch.schema.json';
import ipcSchema from '../ipc.schema.json';
import jobSchema from '../job.schema.json';
import operationSchema from '../operation.schema.json';

const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
ajv.addSchema(ipcSchema);
ajv.addSchema(batchSchema);

const validateJob = ajv.compile(jobSchema);
const validateOperation = ajv.compile(operationSchema);
const validateProgress = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/progressEvent`,
});
const validateJobsCreateRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/jobsCreateRequest`,
});
const validatePdfToImagesRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/pdfToImagesJobCreateRequest`,
});
const validateTextToPdfRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/textToPdfJobCreateRequest`,
});
const validateBatchPreviewRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${batchSchema.$id}#/$defs/batchPreviewRequest`,
  $defs: batchSchema.$defs,
});
const validateBatchCreateRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${batchSchema.$id}#/$defs/batchCreateRequest`,
  $defs: batchSchema.$defs,
});
const validateBatchPreviewResponse = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${batchSchema.$id}#/$defs/batchPreviewResponse`,
  $defs: batchSchema.$defs,
});
const validateBatchRecord = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${batchSchema.$id}#/$defs/batchRecord`,
  $defs: batchSchema.$defs,
});
const validateIpcBatchPreviewRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/batchPreviewRequest`,
});
const validateIpcBatchCreateRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/batchCreateRequest`,
});
const validateIpcBatchGetRequest = ajv.compile({
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: `${ipcSchema.$id}#/$defs/batchGetRequest`,
});
describe('foundation contracts', () => {
  it('accepts the shared golden job, operation, and event', () => {
    expect(fixtures.job.operationVersion).toBe('1.0.1');
    expect(fixtures.operationManifest.version).toBe('1.0.1');
    expect(validateJob(fixtures.job), JSON.stringify(validateJob.errors)).toBe(true);
    expect(validateOperation(fixtures.operationManifest), JSON.stringify(validateOperation.errors)).toBe(true);
    expect(validateProgress(fixtures.progressEvent), JSON.stringify(validateProgress.errors)).toBe(true);
  });

  it.each([
    ['invalid UUID', { ...fixtures.job, id: 'job-1' }],
    ['unknown state', { ...fixtures.job, state: 'done' }],
    ['unsafe extra field', { ...fixtures.job, documentBody: 'never store this' }],
    ['negative progress', { ...fixtures.job, progress: { ...fixtures.job.progress, completedUnits: -1 } }],
  ])('rejects %s', (_label, candidate) => {
    expect(validateJob(candidate)).toBe(false);
  });

  it.each([
    ['legacy null outcome', { completionKind: null, reason: null }],
    ['published outcome', { completionKind: 'published', reason: null }],
    ['no-benefit outcome', { completionKind: 'no-benefit', reason: 'savings-threshold-not-met' }],
  ])('accepts %s', (_label, outcome) => {
    expect(validateJob({ ...fixtures.job, ...outcome }), JSON.stringify(validateJob.errors)).toBe(true);
  });

  it.each([
    ['null kind with reason', { completionKind: null, reason: 'savings-threshold-not-met' }],
    ['published with reason', { completionKind: 'published', reason: 'savings-threshold-not-met' }],
    ['no-benefit without reason', { completionKind: 'no-benefit', reason: null }],
    ['no-benefit with another reason', { completionKind: 'no-benefit', reason: 'candidate-grew' }],
  ])('rejects invalid completion outcome: %s', (_label, outcome) => {
    expect(validateJob({ ...fixtures.job, ...outcome })).toBe(false);
  });

  it('rejects an operation that skips a lifecycle stage', () => {
    const candidate = {
      ...fixtures.operationManifest,
      stages: fixtures.operationManifest.stages.filter((stage) => stage !== 'verify'),
    };
    expect(validateOperation(candidate)).toBe(false);
  });

  it('rejects progress events that leak unapproved fields', () => {
    expect(validateProgress({ ...fixtures.progressEvent, sourcePath: 'C:\\secret.txt' })).toBe(false);
  });
});

describe('text.to-pdf contracts', () => {
  it('accepts only the frozen local TXT manifest and two-setting request', () => {
    expect(validateOperation(textToPdfFixtures.operationManifest), JSON.stringify(validateOperation.errors)).toBe(true);
    expect(validateTextToPdfRequest(textToPdfFixtures.request), JSON.stringify(validateTextToPdfRequest.errors)).toBe(true);
  });

  it.each([
    ['extra setting', { settings: { ...textToPdfFixtures.request.settings, fontSize: 12 } }],
    ['unsupported page size', { settings: { ...textToPdfFixtures.request.settings, pageSize: 'legal' } }],
    ['unsupported orientation', { settings: { ...textToPdfFixtures.request.settings, orientation: 'auto' } }],
    ['non-PDF name', { requestedOutputName: 'notes.txt' }],
    ['raw path', { inputPath: 'C:\\private\\notes.txt' }],
  ])('rejects %s', (_label, patch) => {
    expect(validateTextToPdfRequest({ ...textToPdfFixtures.request, ...patch })).toBe(false);
  });
});

describe('pdf.merge contracts', () => {
  it('accepts the production manifest and minimum ordered request', () => {
    expect(
      validateOperation(pdfMergeFixtures.operationManifest),
      JSON.stringify(validateOperation.errors),
    ).toBe(true);
    expect(
      validateJobsCreateRequest(pdfMergeFixtures.minimumRequest),
      JSON.stringify(validateJobsCreateRequest.errors),
    ).toBe(true);
  });

  it.each([
    ['one input', ['C:\\input\\only.pdf']],
    ['more than 128 inputs', Array.from({ length: 129 }, (_, index) => `C:\\input\\${index}.pdf`)],
  ])('rejects %s', (_label, inputPaths) => {
    expect(validateJobsCreateRequest({
      ...pdfMergeFixtures.minimumRequest,
      inputPaths,
    })).toBe(false);
  });

  it('preserves duplicate paths as intentional ordered entries', () => {
    const duplicate = 'C:\\input\\same.pdf';
    expect(validateJobsCreateRequest({
      ...pdfMergeFixtures.minimumRequest,
      inputPaths: [duplicate, duplicate],
    })).toBe(true);
  });

  it('rejects a non-PDF output name and extra settings', () => {
    expect(validateJobsCreateRequest({
      ...pdfMergeFixtures.minimumRequest,
      requestedOutputName: 'merged.txt',
    })).toBe(false);
    expect(validateJobsCreateRequest({
      ...pdfMergeFixtures.minimumRequest,
      settings: {},
    })).toBe(false);
  });
});

describe('pdf.compress-lossless contracts', () => {
  it('accepts exactly the public v1 manifest and one-PDF request', () => {
    expect(pdfCompressLosslessFixtures.operationManifest.id).toBe('pdf.compress-lossless');
    expect(pdfCompressLosslessFixtures.operationManifest.version).toBe('1.0.0');
    expect(
      validateOperation(pdfCompressLosslessFixtures.operationManifest),
      JSON.stringify(validateOperation.errors),
    ).toBe(true);
    expect(
      validateJobsCreateRequest(pdfCompressLosslessFixtures.request),
      JSON.stringify(validateJobsCreateRequest.errors),
    ).toBe(true);
  });

  it('rejects zero, multiple, non-PDF, and settings-bearing requests', () => {
    for (const candidate of [
      { ...pdfCompressLosslessFixtures.request, inputPaths: [] },
      { ...pdfCompressLosslessFixtures.request, inputPaths: ['C:\\input\\a.pdf', 'C:\\input\\b.pdf'] },
      { ...pdfCompressLosslessFixtures.request, requestedOutputName: 'compressed.txt' },
      { ...pdfCompressLosslessFixtures.request, settings: { quality: 'balanced' } },
    ]) {
      expect(validateJobsCreateRequest(candidate)).toBe(false);
    }
  });
});

describe('pdf.compress-balanced contracts', () => {
  it('accepts only the fixed balanced-v1 request and zero-or-one output manifest', () => {
    expect(
      validateOperation(pdfCompressBalancedFixtures.operationManifest),
      JSON.stringify(validateOperation.errors),
    ).toBe(true);
    expect(pdfCompressBalancedFixtures.operationManifest.outputs.multiplicity).toBe('zero-or-one');
    expect(
      validateJobsCreateRequest(pdfCompressBalancedFixtures.request),
      JSON.stringify(validateJobsCreateRequest.errors),
    ).toBe(true);
  });

  it('rejects zero, multiple, non-PDF, free-form, and missing settings', () => {
    const { settings: _settings, ...withoutSettings } = pdfCompressBalancedFixtures.request;
    for (const candidate of [
      { ...pdfCompressBalancedFixtures.request, inputPaths: [] },
      { ...pdfCompressBalancedFixtures.request, inputPaths: ['C:\\input\\a.pdf', 'C:\\input\\b.pdf'] },
      { ...pdfCompressBalancedFixtures.request, requestedOutputName: 'balanced.txt' },
      { ...pdfCompressBalancedFixtures.request, settings: { profile: 'custom', quality: 70 } },
      withoutSettings,
    ]) {
      expect(validateJobsCreateRequest(candidate)).toBe(false);
    }
  });
});

describe('pdf.to-images contracts', () => {
  it('accepts exactly version 1 with ordered pages, fixed format, and enumerated DPI', () => {
    expect(pdfToImagesFixtures.operationManifest.id).toBe('pdf.to-images');
    expect(pdfToImagesFixtures.operationManifest.version).toBe('1.0.0');
    expect(validateOperation(pdfToImagesFixtures.operationManifest), JSON.stringify(validateOperation.errors)).toBe(true);
    expect(validatePdfToImagesRequest(pdfToImagesFixtures.request), JSON.stringify(validatePdfToImagesRequest.errors)).toBe(true);
  });

  it.each([
    ['free-form DPI', { ...pdfToImagesFixtures.request, dpi: 96 }],
    ['lossy WebP setting', { ...pdfToImagesFixtures.request, webpQuality: 80 }],
    ['129 outputs', { ...pdfToImagesFixtures.request, pages: Array.from({ length: 129 }, (_, sourcePageIndex) => ({ sourcePageIndex, width: 1, height: 1 })), sourcePageCount: 129 }],
    ['duplicate page plan', { ...pdfToImagesFixtures.request, pages: [pdfToImagesFixtures.request.pages[0], pdfToImagesFixtures.request.pages[0]] }],
    ['oversized width', { ...pdfToImagesFixtures.request, pages: [{ sourcePageIndex: 0, width: 8193, height: 1 }] }],
  ])('rejects %s', (_label, candidate) => {
    expect(validatePdfToImagesRequest(candidate)).toBe(false);
  });
});

describe('G04F1 batch preview contracts', () => {
  it('documents Rust as authoritative for the Windows UTF-16 filename limit', () => {
    expect(batchSchema.$defs.pdfName.maxLength).toBe(255);
    expect(batchSchema.$defs.pdfName.$comment).toContain('Rust is authoritative');
    expect(batchSchema.$defs.pdfName.$comment).toContain('UTF-16 code units');
  });

  it('accepts the closed preview, create, response, and durable record shapes', () => {
    expect(
      validateBatchPreviewRequest(batchFixtures.previewRequest),
      JSON.stringify(validateBatchPreviewRequest.errors),
    ).toBe(true);
    expect(validateBatchCreateRequest({
      ...batchFixtures.previewRequest,
      previewSha256: batchFixtures.previewResponse.previewSha256,
      optimisticVersion: batchFixtures.previewResponse.optimisticVersion,
    }), JSON.stringify(validateBatchCreateRequest.errors)).toBe(true);
    expect(
      validateBatchPreviewResponse(batchFixtures.previewResponse),
      JSON.stringify(validateBatchPreviewResponse.errors),
    ).toBe(true);
    expect(
      validateBatchRecord(batchFixtures.batchRecord),
      JSON.stringify(validateBatchRecord.errors),
    ).toBe(true);
    const createRequest = {
      ...batchFixtures.previewRequest,
      previewSha256: batchFixtures.previewResponse.previewSha256,
      optimisticVersion: batchFixtures.previewResponse.optimisticVersion,
    };
    expect(validateIpcBatchPreviewRequest(batchFixtures.previewRequest)).toBe(true);
    expect(validateIpcBatchCreateRequest(createRequest)).toBe(true);
    expect(validateIpcBatchGetRequest({ batchId: batchFixtures.batchRecord.id })).toBe(true);
  });

  it.each([
    ['unknown request field', { ...batchFixtures.previewRequest, requestedNames: ['leak.pdf'] }],
    ['unknown settings field', { ...batchFixtures.previewRequest, settings: { quality: 80 } }],
    ['wrong operation version', { ...batchFixtures.previewRequest, operationVersion: '2.0.0' }],
    ['129 inputs', { ...batchFixtures.previewRequest, inputPaths: Array.from({ length: 129 }, (_, index) => `C:\\private\\${index}.pdf`) }],
  ])('rejects %s', (_label, candidate) => {
    expect(validateBatchPreviewRequest(candidate)).toBe(false);
  });

  it('keeps canonical fingerprints and local paths out of the preview response', () => {
    const serialized = JSON.stringify(batchFixtures.previewResponse);
    for (const forbidden of ['sourcePath', 'destinationDirectory', 'fileIdentity', 'modifiedAt', 'sha256":"aaaa', 'C:\\\\private']) {
      expect(serialized).not.toContain(forbidden);
    }
    expect(validateBatchPreviewResponse({
      ...batchFixtures.previewResponse,
      sourcePath: 'C:\\private\\alpha.pdf',
    })).toBe(false);
  });
});
