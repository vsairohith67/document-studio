import type { JobRecord } from '@document-studio/contracts';

interface PdfToImagesJobLifecycleApi {
  cancel(request: { jobId: string }): Promise<unknown>;
  get(request: { jobId: string }): Promise<JobRecord>;
}

const reconciledCancellationStates = new Set<JobRecord['state']>(['cancelled', 'failed']);

/**
 * Owns exactly one frontend-to-native PDF-to-images operation. A job ID can
 * arrive after the AbortSignal fires, so cancellation intent lives longer
 * than the create promise and is reconciled as soon as ownership registers.
 */
export class PdfToImagesOperation {
  readonly controller = new AbortController();
  private jobId: string | null = null;
  private cancellationRequest: Promise<void> | null = null;
  private reconciliation: Promise<JobRecord> | null = null;
  private frontendCleanup: Promise<void> | null = null;
  private frontendCleanupFailure: unknown = null;
  private frontendSettled = false;

  constructor(private readonly jobs: PdfToImagesJobLifecycleApi) {}

  get signal(): AbortSignal {
    return this.controller.signal;
  }

  get hasCreatedJob(): boolean {
    return this.jobId !== null;
  }

  get isFrontendSettled(): boolean {
    return this.frontendSettled;
  }

  registerCreatedJob(jobId: string): void {
    if (this.jobId !== null) {
      throw new Error('This PDF-to-images operation already owns a native job.');
    }
    this.jobId = jobId;
  }

  ownsJob(jobId: string): boolean {
    return this.jobId === jobId;
  }

  markFrontendSettled(): void {
    this.frontendSettled = true;
  }

  startFrontendCleanup(cleanup: () => Promise<void>): Promise<void> {
    if (this.frontendCleanup === null) {
      this.frontendCleanupFailure = null;
      const pending = cleanup();
      this.frontendCleanup = pending;
      void pending.catch((reason: unknown) => {
        this.frontendCleanupFailure = reason;
        if (this.frontendCleanup === pending) this.frontendCleanup = null;
      });
    }
    return this.frontendCleanup;
  }

  async waitForFrontendCleanup(): Promise<void> {
    if (this.frontendCleanup !== null) await this.frontendCleanup;
    if (this.frontendCleanupFailure !== null) throw this.frontendCleanupFailure;
  }

  async requestCancellation(): Promise<void> {
    this.controller.abort();
    if (this.jobId !== null) {
      await this.ensureCancellationRequested();
    }
  }

  reconcileAfterAbort(): Promise<JobRecord> {
    if (!this.signal.aborted || this.jobId === null) {
      return Promise.reject(new Error('A created PDF-to-images job can only be reconciled after cancellation.'));
    }
    if (this.reconciliation === null) {
      const reconciliation = (async () => {
        await this.ensureCancellationRequested();
        const jobId = this.jobId as string;
        const snapshot = await this.jobs.get({ jobId });
        if (snapshot.id !== jobId || !reconciledCancellationStates.has(snapshot.state)) {
          throw new Error('The cancelled PDF-to-images job has not reached a reconciled terminal state.');
        }
        return snapshot;
      })();
      this.reconciliation = reconciliation;
      void reconciliation.catch(() => {
        if (this.reconciliation === reconciliation) this.reconciliation = null;
      });
    }
    return this.reconciliation;
  }

  private async ensureCancellationRequested(): Promise<void> {
    if (this.jobId === null) return;
    if (this.cancellationRequest === null) {
      const jobId = this.jobId;
      const request = this.jobs.cancel({ jobId }).then(() => undefined);
      this.cancellationRequest = request;
      void request.catch(() => {
        if (this.cancellationRequest === request) this.cancellationRequest = null;
      });
    }
    await this.cancellationRequest;
  }
}
