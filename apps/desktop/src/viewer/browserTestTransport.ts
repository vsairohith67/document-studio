import type {
  CorePdfJobCreateRequest,
  DestinationGrant,
  JobRecord,
  ProgressEvent,
  ViewerDocumentMetadata,
  ViewerRangeRequest,
  ViewerSessionRequest,
} from '@document-studio/contracts';

export interface BrowserTestTransport {
  open(): Promise<ViewerDocumentMetadata | null>;
  readRange(request: ViewerRangeRequest): Promise<Uint8Array>;
  close(request: ViewerSessionRequest): Promise<void>;
  setDropEnabled(enabled: boolean): Promise<void>;
  chooseDestination(): Promise<DestinationGrant | null>;
  revokeDestination(grantId: string): Promise<void>;
  createCorePdf(request: CorePdfJobCreateRequest): Promise<JobRecord>;
  onProgress?(handler: (event: ProgressEvent) => void): Promise<() => void>;
}

declare global {
  var __DOCUMENT_STUDIO_G03_TEST_TRANSPORT__: BrowserTestTransport | undefined;
}

export function browserTestTransport(): BrowserTestTransport | undefined {
  return browserTestMode()
    ? globalThis.__DOCUMENT_STUDIO_G03_TEST_TRANSPORT__
    : undefined;
}

export function browserTestMode(): boolean {
  return import.meta.env.MODE === 'test-browser';
}
