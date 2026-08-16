import Ajv2020 from 'ajv/dist/2020';
import addFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';
import fixtures from '../fixtures/foundation-contracts.json';
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
describe('foundation contracts', () => {
  it('accepts the shared golden job, operation, and event', () => {
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
