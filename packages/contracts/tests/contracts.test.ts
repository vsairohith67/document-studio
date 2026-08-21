import Ajv2020 from 'ajv/dist/2020';
import addFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';
import fixtures from '../fixtures/foundation-contracts.json';
import pdfMergeFixtures from '../fixtures/pdf-merge-contracts.json';
import pdfCompressLosslessFixtures from '../fixtures/pdf-compress-lossless-contracts.json';
import ipcSchema from '../ipc.schema.json';
import jobSchema from '../job.schema.json';
import operationSchema from '../operation.schema.json';

const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
ajv.addSchema(ipcSchema);

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
