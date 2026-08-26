import type { JobRecord, JobState, ProgressEvent } from '@document-studio/contracts';

interface BalancedCompressionLifecycleApi {
  cancel(request: { jobId: string }): Promise<unknown>;
  get(request: { jobId: string }): Promise<JobRecord>;
}

type CreateState = 'idle' | 'pending' | 'settled' | 'failed';

const terminalStates = new Set<JobState>(['completed', 'failed', 'cancelled', 'interrupted']);

function cancellationIsTooLate(error: unknown): boolean {
  return typeof error === 'object'
    && error !== null
    && 'code' in error
    && error.code === 'CANCEL_TOO_LATE';
}

/**
 * Owns exactly one balanced frontend/native operation. Dispose intent outlives
 * a pending native create promise, while job and generation checks keep late
 * events and supplemental audit reads from mutating a replacement operation.
 */
export class BalancedCompressionOperation {
  readonly isBalanced = true;
  private ownedJob: JobRecord | null = null;
  private ownedJobId: string | null = null;
  private latestSequence = -1;
  private state: JobState | null = null;
  private create: CreateState = 'idle';
  private disposeRequested = false;
  private cancellationRequest: Promise<JobRecord | null> | null = null;
  private cancellationRequested = false;

  constructor(
    readonly generation: number,
    private readonly jobs: BalancedCompressionLifecycleApi,
  ) {}

  get createState(): CreateState {
    return this.create;
  }

  get latestState(): JobState | null {
    return this.state;
  }

  get isDisposed(): boolean {
    return this.disposeRequested;
  }

  get cancellationWasRequested(): boolean {
    return this.cancellationRequested;
  }

  get canBeReplaced(): boolean {
    return this.create === 'failed'
      || (this.state !== null && terminalStates.has(this.state));
  }

  beginCreate(): void {
    if (this.create !== 'idle') {
      throw new Error('This balanced operation already started native creation.');
    }
    this.create = 'pending';
  }

  failCreate(): void {
    if (this.ownedJobId !== null) {
      throw new Error('A created balanced job cannot be changed to create-failed.');
    }
    this.create = 'failed';
  }

  registerCreatedJob(created: JobRecord): Promise<JobRecord | null> {
    if (created.operationId !== 'pdf.compress-balanced') {
      throw new Error('The balanced operation received a job for another operation.');
    }
    if (this.ownedJobId !== null) {
      throw new Error('This balanced operation already owns a native job.');
    }
    this.create = 'settled';
    this.ownedJobId = created.id;
    this.observeJob(created);
    return this.disposeRequested ? this.reconcileOwnedJob() : Promise.resolve(null);
  }

  ownsJob(jobId: string): boolean {
    return this.ownedJobId === jobId;
  }

  acceptsCallback(generation: number, jobId: string): boolean {
    return !this.disposeRequested
      && generation === this.generation
      && this.ownsJob(jobId);
  }

  canStartVisual(jobId: string): boolean {
    return !this.disposeRequested
      && !this.cancellationRequested
      && this.ownsJob(jobId)
      && this.state !== 'publishing'
      && (this.state === null || !terminalStates.has(this.state));
  }

  observeProgress(event: ProgressEvent): boolean {
    if (!this.ownsJob(event.jobId) || event.sequence <= this.latestSequence) return false;
    this.latestSequence = event.sequence;
    this.state = event.state;
    return true;
  }

  observeJob(snapshot: JobRecord): boolean {
    if (!this.ownsJob(snapshot.id) || snapshot.sequence < this.latestSequence) return false;
    this.latestSequence = snapshot.sequence;
    this.state = snapshot.state;
    this.ownedJob = snapshot;
    return true;
  }

  dispose(): Promise<JobRecord | null> {
    this.disposeRequested = true;
    return this.reconcileOwnedJob();
  }

  reconcileOwnedJob(): Promise<JobRecord | null> {
    if (this.ownedJobId === null) return Promise.resolve(null);
    if (this.state === 'publishing' || (this.state !== null && terminalStates.has(this.state))) {
      return Promise.resolve(this.ownedJob);
    }
    if (this.cancellationRequest === null) {
      const jobId = this.ownedJobId;
      const sendCancellation = !this.cancellationRequested;
      if (sendCancellation) this.cancellationRequested = true;
      let request!: Promise<JobRecord | null>;
      request = (async () => {
        try {
          if (sendCancellation) await this.jobs.cancel({ jobId });
        } catch (reason) {
          const snapshot = await this.jobs.get({ jobId }).catch(() => null);
          if (snapshot) this.observeJob(snapshot);
          if (
            snapshot
            && (snapshot.state === 'publishing' || terminalStates.has(snapshot.state))
          ) {
            return snapshot;
          }
          if (cancellationIsTooLate(reason) && snapshot) return snapshot;
          this.cancellationRequested = false;
          this.cancellationRequest = null;
          throw reason;
        }
        try {
          const snapshot = await this.jobs.get({ jobId });
          this.observeJob(snapshot);
          return snapshot;
        } finally {
          if (
            this.cancellationRequest === request
            && this.state !== null
            && !terminalStates.has(this.state)
            && this.state !== 'publishing'
          ) {
            this.cancellationRequest = null;
          }
        }
      })();
      this.cancellationRequest = request;
    }
    return this.cancellationRequest;
  }
}
