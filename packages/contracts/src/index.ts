export type JobState =
  | 'queued' | 'inspecting' | 'preflight' | 'ready' | 'running'
  | 'verifying' | 'publishing' | 'completed' | 'failed'
  | 'cancelled' | 'interrupted';

export interface ProgressEvent {
  jobId: string;
  operationId: string;
  state: JobState;
  stage: string;
  completedUnits: number;
  totalUnits?: number;
  message: string;
  cancellable: boolean;
}

export interface OperationError {
  code: string;
  title: string;
  detail: string;
  stage: string;
  retryable: boolean;
  inputIndex?: number;
  helpId?: string;
}
