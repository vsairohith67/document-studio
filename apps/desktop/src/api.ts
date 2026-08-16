import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  CancelResponse,
  DependencyDiagnostic,
  FileInspection,
  HistoryDeleteRequest,
  HistoryListRequest,
  JobIdRequest,
  JobRecord,
  JobsCreateRequest,
  OperationManifest,
  ProgressEvent,
  SettingGetRequest,
  SettingRecord,
  SettingSetRequest,
  SystemStatus,
} from '@document-studio/contracts';
import { JOB_PROGRESS_EVENT_NAME } from '@document-studio/contracts';

export const api = {
  system: {
    status: () => invoke<SystemStatus>('system_status'),
  },
  operations: {
    list: () => invoke<OperationManifest[]>('operations_list'),
  },
  files: {
    inspect: (paths: string[]) =>
      invoke<FileInspection[]>('files_inspect', { request: { paths } }),
  },
  dialogs: {
    async selectInput(): Promise<string | null> {
      const selected = await open({ directory: false, multiple: false });
      return typeof selected === 'string' ? selected : null;
    },
    async selectDestination(): Promise<string | null> {
      const selected = await open({ directory: true, multiple: false });
      return typeof selected === 'string' ? selected : null;
    },
  },
  jobs: {
    create: (request: JobsCreateRequest) =>
      invoke<JobRecord>('jobs_create', { request }),
    cancel: (request: JobIdRequest) =>
      invoke<CancelResponse>('jobs_cancel', { request }),
    resolveInterrupted: (request: JobIdRequest) =>
      invoke<JobRecord>('jobs_resolve_interrupted', { request }),
    get: (request: JobIdRequest) => invoke<JobRecord>('jobs_get', { request }),
    onProgress: (handler: (event: ProgressEvent) => void): Promise<UnlistenFn> =>
      listen<ProgressEvent>(JOB_PROGRESS_EVENT_NAME, (event) => handler(event.payload)),
  },
  history: {
    list: (request: HistoryListRequest) =>
      invoke<JobRecord[]>('history_list', { request }),
    delete: (request: HistoryDeleteRequest) =>
      invoke<number>('history_delete', { request }),
  },
  dependencies: {
    scan: () => invoke<DependencyDiagnostic[]>('dependencies_scan'),
  },
  settings: {
    get: (request: SettingGetRequest) =>
      invoke<SettingRecord | null>('settings_get', { request }),
    set: <T>(request: SettingSetRequest<T>) =>
      invoke<SettingRecord<T>>('settings_set', { request }),
  },
};

export function createProgressReconciler(
  fetchJob: (jobId: string) => Promise<JobRecord>,
  onSnapshot: (job: JobRecord) => void,
  onEvent: (event: ProgressEvent) => void,
) {
  const latestSequence = new Map<string, number>();
  return async (event: ProgressEvent): Promise<void> => {
    const previous = latestSequence.get(event.jobId);
    if (previous !== undefined && event.sequence <= previous) {
      return;
    }
    if (previous !== undefined && event.sequence > previous + 1) {
      onSnapshot(await fetchJob(event.jobId));
    }
    latestSequence.set(event.jobId, event.sequence);
    onEvent(event);
  };
}
export function operationErrorMessage(error: unknown): string {
  if (
    typeof error === 'object' &&
    error !== null &&
    'title' in error &&
    'detail' in error &&
    typeof error.title === 'string' &&
    typeof error.detail === 'string'
  ) {
    return `${error.title}. ${error.detail}`;
  }
  return 'Document Studio could not complete that request.';
}
