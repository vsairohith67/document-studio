import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  CancelResponse,
  CorePdfJobCreateRequest,
  DependencyDiagnostic,
  DestinationGrant,
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
  ViewerDocumentMetadata,
  ViewerRangeRequest,
  ViewerSessionRequest,
} from '@document-studio/contracts';
import { JOB_PROGRESS_EVENT_NAME } from '@document-studio/contracts';
import { browserTestMode, browserTestTransport } from './viewer/browserTestTransport';

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
    onPdfDrop: (handler: (paths: string[]) => void): Promise<UnlistenFn> =>
      browserTestMode()
        ? Promise.resolve(() => undefined)
        : getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type === 'drop') handler(event.payload.paths);
        }),
  },
  viewer: {
    open: () => browserTestTransport()?.open() ?? invoke<ViewerDocumentMetadata | null>('viewer_open_dialog'),
    async readRange(request: ViewerRangeRequest): Promise<Uint8Array> {
      const testTransport = browserTestTransport();
      if (testTransport) return testTransport.readRange(request);
      // Tauri's raw octet-stream response can surface as an ArrayBuffer, a
      // typed view, or a number array depending on the WebView bridge. The
      // Rust command still returns `tauri::ipc::Response`; none of these forms
      // use Document Studio's JSON command serializer or base64.
      const bytes = await invoke<ArrayBuffer | Uint8Array | number[]>('viewer_read_range', { request });
      if (bytes instanceof Uint8Array) return bytes;
      if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
      if (Array.isArray(bytes) && bytes.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
        return new Uint8Array(bytes);
      }
      throw new Error('The native viewer range response was not raw byte data.');
    },
    close: (request: ViewerSessionRequest) =>
      browserTestTransport()?.close(request) ?? invoke<void>('viewer_close', { request }),
    setDropEnabled: (enabled: boolean) =>
      browserTestTransport()?.setDropEnabled(enabled) ?? invoke<void>('viewer_set_drop_enabled', { enabled }),
    chooseDestination: () =>
      browserTestTransport()?.chooseDestination() ?? invoke<DestinationGrant | null>('viewer_choose_destination'),
    revokeDestination: (grantId: string) =>
      browserTestTransport()?.revokeDestination(grantId)
      ?? invoke<void>('viewer_revoke_destination', { request: { grantId } }),
    onDocumentOpened: (handler: (document: ViewerDocumentMetadata) => void): Promise<UnlistenFn> =>
      browserTestTransport()
        ? Promise.resolve(() => undefined)
        : listen<ViewerDocumentMetadata>('document-studio-viewer-document-opened-v1', (event) =>
          handler(event.payload)),
    onOpenFailed: (handler: (error: unknown) => void): Promise<UnlistenFn> =>
      browserTestTransport()
        ? Promise.resolve(() => undefined)
        : listen('document-studio-viewer-open-failed-v1', (event) => handler(event.payload)),
  },
  dialogs: {
    async selectPdfInputs(): Promise<string[]> {
      const selected = await open({
        directory: false,
        multiple: true,
        filters: [{ name: 'PDF documents', extensions: ['pdf'] }],
      });
      if (Array.isArray(selected)) return selected;
      return typeof selected === 'string' ? [selected] : [];
    },
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
    createCorePdf: (request: CorePdfJobCreateRequest) =>
      browserTestTransport()?.createCorePdf(request)
      ?? invoke<JobRecord>('jobs_create_core_pdf', { request }),
    cancel: (request: JobIdRequest) =>
      invoke<CancelResponse>('jobs_cancel', { request }),
    resolveInterrupted: (request: JobIdRequest) =>
      invoke<JobRecord>('jobs_resolve_interrupted', { request }),
    get: (request: JobIdRequest) => invoke<JobRecord>('jobs_get', { request }),
    onProgress: (handler: (event: ProgressEvent) => void): Promise<UnlistenFn> =>
      browserTestTransport()?.onProgress?.(handler)
      ?? (browserTestMode()
        ? Promise.resolve(() => undefined)
        : listen<ProgressEvent>(JOB_PROGRESS_EVENT_NAME, (event) => handler(event.payload))),
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
