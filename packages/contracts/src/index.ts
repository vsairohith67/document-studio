export const JOB_STATES = [
  'queued',
  'inspecting',
  'preflight',
  'ready',
  'running',
  'verifying',
  'publishing',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
] as const;

export type JobState = (typeof JOB_STATES)[number];

export const OPERATION_STAGES = [
  'inspect',
  'preflight',
  'estimate',
  'plan',
  'execute',
  'verify',
  'publish',
  'audit',
  'cleanup',
  'recovery',
] as const;

export type OperationStage = (typeof OPERATION_STAGES)[number];
export type ProgressUnit = 'bytes' | 'items' | 'steps';
export type OperationRisk = 'normal' | 'sensitive' | 'irreversible';
export type OperationLocality = 'local' | 'local-or-cloud' | 'cloud';
export type DependencyStatus = 'available' | 'missing' | 'unhealthy' | 'deferred' | 'not-required';
export type OutputStatus = 'planned' | 'staged' | 'verified' | 'publishing' | 'published';

export interface OperationError {
  code: string;
  title: string;
  detail: string;
  stage: OperationStage;
  retryable: boolean;
  inputIndex?: number;
  helpId?: string;
}

export interface JobProgress {
  completedUnits: number;
  totalUnits: number;
  unit: ProgressUnit;
}

export interface JobInput {
  ordinal: number;
  displayName: string;
  sourcePath: string;
  canonicalPath: string;
  fileIdentity: string;
  sizeBytes: number;
  modifiedAt: string;
  mimeType: string;
  sha256: string | null;
  passwordReference: string | null;
}

export interface JobOutput {
  ordinal: number;
  requestedName: string;
  resolvedName: string | null;
  stagingPath: string | null;
  partialPath: string | null;
  finalPath: string | null;
  sizeBytes: number | null;
  mimeType: string;
  sha256: string | null;
  status: OutputStatus;
  verifiedAt: string | null;
  publishedAt: string | null;
}

export interface JobRecord {
  id: string;
  operationId: string;
  operationVersion: string;
  state: JobState;
  stage: OperationStage | null;
  sequence: number;
  progress: JobProgress;
  destinationDirectory: string;
  requestedOutputName: string;
  resolvedOutputName: string | null;
  cancellationRequestedAt: string | null;
  createdAt: string;
  updatedAt: string;
  finishedAt: string | null;
  version: number;
  inputs: JobInput[];
  outputs: JobOutput[];
  errors: OperationError[];
}

export interface ProgressEvent {
  schemaVersion: 1;
  sequence: number;
  emittedAt: string;
  jobId: string;
  operationId: string;
  state: JobState;
  stage: OperationStage;
  completedUnits: number;
  totalUnits: number;
  unit: ProgressUnit;
  messageCode: string;
  message: string;
  cancellable: boolean;
}

export interface DependencyDiagnostic {
  id: string;
  kind: 'built-in' | 'external' | 'deferred';
  status: DependencyStatus;
  version: string | null;
  capabilities: string[];
  checkedAt: string;
  errorCode: string | null;
}

export interface FileInspection {
  path: string;
  displayName: string;
  sizeBytes: number;
  modifiedAt: string;
  mimeType: string;
  fileIdentity: string;
}

export interface SystemStatus {
  product: string;
  phase: string;
  offlineByDefault: boolean;
  databaseSchemaVersion: number;
}

export interface CancelResponse {
  outcome: 'requested' | 'cancelled';
}

export interface SettingRecord<T = unknown> {
  scope: 'application' | 'operation';
  key: string;
  value: T;
  version: number;
  updatedAt: string;
}

export interface OperationManifest {
  id: string;
  version: string;
  name: string;
  category: string;
  description: string;
  risk: OperationRisk;
  locality: OperationLocality;
  inputs: {
    acceptedMimeTypes: string[];
    minimum: number;
    maximum: number;
    allowDirectories: boolean;
  };
  settingsSchema: Record<string, unknown>;
  outputs: {
    mimeType: string;
    multiplicity: 'single' | 'multiple';
  };
  dependencies: string[];
  verification: string[];
  stages: OperationStage[];
}

export interface FilesInspectRequest {
  paths: string[];
}

export interface DiagnosticCopyCreateRequest {
  operationId: 'diagnostic.copy';
  inputPaths: [string];
  destinationDirectory: string;
  requestedOutputName: string;
}

export interface PdfMergeCreateRequest {
  operationId: 'pdf.merge';
  inputPaths: [string, string, ...string[]];
  destinationDirectory: string;
  requestedOutputName: string;
}

export type JobsCreateRequest = DiagnosticCopyCreateRequest | PdfMergeCreateRequest;

export const CORE_PDF_OPERATION_IDS = [
  'pdf.extract-pages',
  'pdf.remove-pages',
  'pdf.reorder-pages',
  'pdf.rotate-pages',
  'pdf.split',
] as const;

export type CorePdfOperationId = (typeof CORE_PDF_OPERATION_IDS)[number];
export type OutputRotation = 90 | 180 | 270;

export interface ExtractPagesPlan {
  selectedPageIndexes: number[];
  outputName: string;
}

export interface RemovePagesPlan {
  removedPageIndexes: number[];
  outputName: string;
}

export interface ReorderPagesPlan {
  orderedPageIndexes: number[];
  outputName: string;
}

export interface RotatePagesPlan {
  rotations: Array<{ pageIndex: number; clockwiseDegrees: OutputRotation }>;
  outputName: string;
}

export interface SplitOutputRange {
  startPageIndex: number;
  endPageIndex: number;
  outputName: string;
}

export interface SplitPlan {
  ranges: SplitOutputRange[];
}

export type CorePdfPlanPayload =
  | ExtractPagesPlan
  | RemovePagesPlan
  | ReorderPagesPlan
  | RotatePagesPlan
  | SplitPlan;

export interface OperationPlanEnvelope<TPayload extends CorePdfPlanPayload = CorePdfPlanPayload> {
  schemaVersion: 1;
  operationId: CorePdfOperationId;
  sourcePageCount: number;
  payload: TPayload;
}

export interface ViewerDocumentMetadata {
  sessionId: string;
  generation: number;
  displayName: string;
  sizeBytes: number;
  modifiedAt: string;
  mimeType: 'application/pdf';
  fileIdentity: string;
}

export interface ViewerRangeRequest {
  sessionId: string;
  generation: number;
  begin: number;
  end: number;
}

export interface ViewerSessionRequest {
  sessionId: string;
  generation: number;
}

export interface DestinationGrant {
  grantId: string;
  displayName: string;
}

export interface DestinationGrantRequest {
  grantId: string;
}

export interface CorePdfJobCreateRequest {
  viewerSessionId: string;
  viewerGeneration: number;
  destinationGrantId: string;
  plan: OperationPlanEnvelope;
}

export interface JobIdRequest {
  jobId: string;
}

export interface HistoryListRequest {
  limit: number;
  beforeUpdatedAt?: string;
}

export interface HistoryDeleteRequest {
  jobIds: string[];
}

export interface SettingGetRequest {
  scope: SettingRecord['scope'];
  key: string;
}

export interface SettingSetRequest<T = unknown> extends SettingGetRequest {
  value: T;
  expectedVersion: number;
}

export type CommandResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: OperationError };

export const JOB_PROGRESS_EVENT_NAME = 'document-studio-job-progress-v1';
export const VIEWER_DOCUMENT_OPENED_EVENT_NAME = 'document-studio-viewer-opened-v1';
