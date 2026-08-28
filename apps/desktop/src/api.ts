import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  BalancedCompressionAudit,
  BalancedCompressionJobCreateRequest,
  BalancedCompressionVisualSession,
  BalancedRenderPageTicket,
  BalancedRenderSide,
  BatchCreateRequest,
  BatchGetRequest,
  BatchPreviewRequest,
  BatchPreviewResponse,
  BatchRecord,
  CancelResponse,
  CorePdfJobCreateRequest,
  DependencyDiagnostic,
  DestinationGrant,
  FileInspection,
  HistoryDeleteRequest,
  HistoryListRequest,
  JobIdRequest,
  JobRecord,
  JobWarning,
  JobsCreateRequest,
  OperationManifest,
  PdfPixelTransferTicket,
  PdfToImagesJobCreateRequest,
  PdfToImagesJobSession,
  ProgressEvent,
  SettingGetRequest,
  SettingRecord,
  SettingSetRequest,
  SystemStatus,
  TextInputMetadata,
  TextToPdfJobCreateRequest,
  ViewerDocumentMetadata,
  ViewerRangeRequest,
  ViewerSessionRequest,
} from '@document-studio/contracts';
import {
  BALANCED_VISUAL_READY_EVENT_NAME,
  JOB_PROGRESS_EVENT_NAME,
} from '@document-studio/contracts';
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
  text: {
    open: () => invoke<TextInputMetadata | null>('text_open_dialog'),
    openOutput: (jobId: string) =>
      invoke<ViewerDocumentMetadata>('jobs_open_text_output', { request: { jobId } }),
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
    async selectImageInputs(): Promise<string[]> {
      const selected = await open({
        directory: false,
        multiple: true,
        filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png', 'webp'] }],
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
  batches: {
    preview: (request: BatchPreviewRequest) =>
      invoke<BatchPreviewResponse>('batches_preview', { request }),
    create: (request: BatchCreateRequest) =>
      invoke<BatchRecord>('batches_create', { request }),
    get: (request: BatchGetRequest) =>
      invoke<BatchRecord>('batches_get', { request }),
  },
  jobs: {
    create: (request: JobsCreateRequest) =>
      invoke<JobRecord>('jobs_create', { request }),
    createCorePdf: (request: CorePdfJobCreateRequest) =>
      browserTestTransport()?.createCorePdf(request)
      ?? invoke<JobRecord>('jobs_create_core_pdf', { request }),
    createPdfToImages: (request: PdfToImagesJobCreateRequest) => {
      const transport = browserTestTransport();
      if (transport?.createPdfToImages) return transport.createPdfToImages(request);
      return invoke<PdfToImagesJobSession>('jobs_create_pdf_to_images', { request });
    },
    createBalanced: (request: BalancedCompressionJobCreateRequest) =>
      invoke<JobRecord>('jobs_create_balanced', { request }),
    createTextToPdf: (request: TextToPdfJobCreateRequest) =>
      invoke<JobRecord>('jobs_create_text_to_pdf', { request }),
    submitBalancedPixels(
      session: BalancedCompressionVisualSession,
      ticket: BalancedRenderPageTicket,
      side: BalancedRenderSide,
      width: number,
      height: number,
      rgba: Uint8Array,
    ): Promise<JobRecord> {
      return invoke<JobRecord>('balanced_compression_submit_page', rgba, {
        headers: {
          'x-document-studio-job-id': session.jobId,
          'x-document-studio-render-session-id': session.renderSessionId,
          'x-document-studio-page-ordinal': String(ticket.pageOrdinal),
          'x-document-studio-source-page-index': String(ticket.sourcePageIndex),
          'x-document-studio-page-nonce': ticket.nonce,
          'x-document-studio-render-side': side,
          'x-document-studio-expected-width': String(width),
          'x-document-studio-expected-height': String(height),
        },
      });
    },
    balancedAudit: (request: JobIdRequest) =>
      invoke<BalancedCompressionAudit | null>('jobs_balanced_audit', { request }),
    onBalancedVisualReady: (
      handler: (session: BalancedCompressionVisualSession) => void,
    ): Promise<UnlistenFn> => listen<BalancedCompressionVisualSession>(
      BALANCED_VISUAL_READY_EVENT_NAME,
      (event) => handler(event.payload),
    ),
    submitPdfPixels(
      session: PdfToImagesJobSession,
      ticket: PdfPixelTransferTicket,
      rgba: Uint8Array,
    ): Promise<JobRecord> {
      const transport = browserTestTransport();
      if (transport?.submitPdfPixels) return transport.submitPdfPixels(session, ticket, rgba);
      return invoke<JobRecord>('pdf_to_images_submit_page', rgba, {
        headers: {
          'x-document-studio-job-id': session.job.id,
          'x-document-studio-render-session-id': session.renderSessionId,
          'x-document-studio-page-ordinal': String(ticket.pageOrdinal),
          'x-document-studio-page-nonce': ticket.nonce,
          'x-document-studio-expected-width': String(ticket.expectedWidth),
          'x-document-studio-expected-height': String(ticket.expectedHeight),
        },
      });
    },
    cancel: (request: JobIdRequest) =>
      invoke<CancelResponse>('jobs_cancel', { request }),
    resolveInterrupted: (request: JobIdRequest) =>
      invoke<JobRecord>('jobs_resolve_interrupted', { request }),
    get: (request: JobIdRequest) => invoke<JobRecord>('jobs_get', { request }),
    warnings: (request: JobIdRequest) => invoke<JobWarning[]>('jobs_warnings', { request }),
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
