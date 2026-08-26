import { useEffect, useRef, useState } from 'react';
import type {
  BalancedCompressionAudit,
  BalancedCompressionVisualSession,
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';
import type { RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { api, createProgressReconciler, operationErrorMessage } from './api';
import { NoBenefitResult } from './JobCompletionOutcome';
import { renderBalancedCompression } from './viewer/balancedCompression';
import { BalancedCompressionOperation } from './viewer/balancedCompressionLifecycle';
import {
  calculateSizeReport,
  formatBytes,
  formatSignedBytes,
  formatSignedPercentage,
  sizeOutcomeLabel,
} from './sizeReporting';

const terminalStates = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

function validPdfOutputName(name: string): boolean {
  return name.length > 4 && name.length <= 255
    && name.toLocaleLowerCase().endsWith('.pdf')
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(name)
    && !/[. ]$/u.test(name)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(name);
}

function compressedName(name: string): string {
  return `${name.replace(/\.pdf$/iu, '')}-compressed.pdf`;
}

interface OptimizeWorkspaceProps {
  system: SystemStatus | null;
  dependencies: DependencyDiagnostic[];
  onOpenMerge: () => void;
  onOpenViewer: () => void;
  onOpenConvert?: () => void;
}

export function OptimizeWorkspace({
  system,
  dependencies,
  onOpenMerge,
  onOpenViewer,
  onOpenConvert = () => undefined,
}: OptimizeWorkspaceProps) {
  const [profile, setProfile] = useState<'lossless' | 'balanced'>('lossless');
  const [source, setSource] = useState<FileInspection | null>(null);
  const [destination, setDestination] = useState<string | null>(null);
  const [outputName, setOutputName] = useState('compressed.pdf');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const [audit, setAudit] = useState<BalancedCompressionAudit | null>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const result = useRef<HTMLDivElement>(null);
  const jobId = useRef<string | null>(null);
  const renderAbort = useRef<AbortController | null>(null);
  const renderTask = useRef<RenderTask | null>(null);
  const pendingVisual = useRef(new Map<string, {
    generation: number;
    session: BalancedCompressionVisualSession;
  }>());
  const visualOwner = useRef<{ generation: number; jobId: string } | null>(null);
  const mounted = useRef(true);
  const operationGeneration = useRef(0);
  const balancedOperation = useRef<BalancedCompressionOperation | null>(null);

  const ownsCurrentBalancedOperation = (operation: BalancedCompressionOperation) => (
    mounted.current
    && balancedOperation.current === operation
    && operation.generation === operationGeneration.current
  );

  const acceptsBalancedCallback = (
    operation: BalancedCompressionOperation,
    callbackJobId: string,
  ) => ownsCurrentBalancedOperation(operation)
    && operation.acceptsCallback(operationGeneration.current, callbackJobId);

  const cancelVisualResources = () => {
    const controller = renderAbort.current;
    if (controller) {
      if (!controller.signal.aborted) controller.abort();
    } else {
      renderTask.current?.cancel();
    }
  };

  const loadBalancedAudit = async (
    completed: JobRecord,
    operation: BalancedCompressionOperation,
  ) => {
    try {
      const value = await api.jobs.balancedAudit({ jobId: completed.id });
      if (acceptsBalancedCallback(operation, completed.id)) setAudit(value);
    } catch {
      // The durable terminal JobRecord is authoritative. Audit is display-only evidence.
    }
  };

  const runBalancedVisual = async (
    session: BalancedCompressionVisualSession,
    operation: BalancedCompressionOperation,
  ) => {
    if (
      !acceptsBalancedCallback(operation, session.jobId)
      || !operation.canStartVisual(session.jobId)
    ) return;
    const activeVisual = visualOwner.current;
    if (activeVisual) {
      if (
        activeVisual.generation !== operation.generation
        || activeVisual.jobId !== session.jobId
      ) {
        pendingVisual.current.set(session.jobId, {
          generation: operation.generation,
          session,
        });
      }
      return;
    }
    const owner = { generation: operation.generation, jobId: session.jobId };
    visualOwner.current = owner;
    pendingVisual.current.delete(session.jobId);
    const controller = new AbortController();
    renderAbort.current = controller;
    try {
      setAnnouncement(`Verifying ${session.pages.length} affected page${session.pages.length === 1 ? '' : 's'} at 144 DPI.`);
      const completed = await renderBalancedCompression(session, controller.signal, {
        onRenderTask: (task) => { renderTask.current = task; },
        onJob: (snapshot) => {
          operation.observeJob(snapshot);
          if (acceptsBalancedCallback(operation, snapshot.id)) setJob(snapshot);
        },
      });
      operation.observeJob(completed);
      if (!acceptsBalancedCallback(operation, completed.id)) return;
      setJob(completed);
      setBusy(false);
      setAnnouncement(completed.completionKind === 'no-benefit'
        ? 'Balanced verification passed, but the candidate did not save both 5% and 64 KiB. No output was created.'
        : 'Balanced verification passed and one verified PDF was published.');
      void loadBalancedAudit(completed, operation);
    } catch (reason) {
      const shouldUpdate = ownsCurrentBalancedOperation(operation);
      const snapshot = await operation.dispose().catch(() => null);
      if (shouldUpdate && mounted.current && snapshot) setJob(snapshot);
      if (shouldUpdate && mounted.current && !controller.signal.aborted) {
        setError(operationErrorMessage(reason));
      }
      if (shouldUpdate && mounted.current) setBusy(false);
    } finally {
      if (renderAbort.current === controller) {
        renderTask.current = null;
        renderAbort.current = null;
      }
      if (visualOwner.current === owner) visualOwner.current = null;
      const nextOperation = balancedOperation.current;
      const nextJobId = jobId.current;
      const pending = nextJobId ? pendingVisual.current.get(nextJobId) : undefined;
      if (
        mounted.current
        && nextOperation
        && nextJobId
        && pending?.generation === nextOperation.generation
        && nextOperation.canStartVisual(pending.session.jobId)
      ) {
        pendingVisual.current.delete(nextJobId);
        void runBalancedVisual(pending.session, nextOperation);
      }
    }
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const reconcile = createProgressReconciler(
      (id) => api.jobs.get({ jobId: id }),
      (snapshot) => {
        const operation = balancedOperation.current;
        if (snapshot.operationId === 'pdf.compress-balanced') {
          if (!operation?.ownsJob(snapshot.id)) return;
          operation.observeJob(snapshot);
          if (active && acceptsBalancedCallback(operation, snapshot.id)) setJob(snapshot);
          return;
        }
        if (active && mounted.current && snapshot.id === jobId.current) setJob(snapshot);
      },
      (event) => {
        if (!active || !mounted.current || event.jobId !== jobId.current) return;
        const operation = balancedOperation.current;
        const balancedEvent = event.operationId === 'pdf.compress-balanced';
        if (balancedEvent) {
          if (!operation?.ownsJob(event.jobId)) return;
          if (!operation.observeProgress(event)) return;
          if (!acceptsBalancedCallback(operation, event.jobId)) return;
        }
        setProgress(event);
        setAnnouncement(event.message);
        if (terminalStates.has(event.state)) {
          setBusy(false);
          void api.jobs.get({ jobId: event.jobId }).then((snapshot) => {
            if (!active || !mounted.current) return;
            if (snapshot.operationId === 'pdf.compress-balanced') {
              if (
                !operation
                || balancedOperation.current !== operation
                || !operation.ownsJob(snapshot.id)
              ) return;
              operation.observeJob(snapshot);
              if (!acceptsBalancedCallback(operation, snapshot.id)) return;
            } else if (snapshot.id !== jobId.current) {
              return;
            }
            setJob(snapshot);
            if (
              snapshot.operationId === 'pdf.compress-balanced'
              && typeof api.jobs.balancedAudit === 'function'
              && operation
            ) {
              void loadBalancedAudit(snapshot, operation);
            }
          }).catch((reason: unknown) => {
            if (active && mounted.current) setError(operationErrorMessage(reason));
          });
        }
      },
    );
    void api.jobs.onProgress((event) => {
      if (event.jobId === jobId.current) {
        void reconcile(event).catch((reason: unknown) => {
          if (active && mounted.current) setError(operationErrorMessage(reason));
        });
      }
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);

  useEffect(() => {
    if (typeof api.jobs.onBalancedVisualReady !== 'function') return undefined;
    let active = true;
    let unlisten: (() => void) | undefined;
    void api.jobs.onBalancedVisualReady((session) => {
      if (!active) return;
      const operation = balancedOperation.current;
      if (!operation || operation.isDisposed) return;
      if (operation.canStartVisual(session.jobId)) {
        void runBalancedVisual(session, operation);
      } else if (
        operation.createState === 'pending'
        && operation.generation === operationGeneration.current
      ) {
        pendingVisual.current.set(session.jobId, {
          generation: operation.generation,
          session,
        });
      }
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      cancelVisualResources();
      const operation = balancedOperation.current;
      if (operation) void operation.dispose().catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (job?.state === 'completed') result.current?.focus();
  }, [job?.state]);

  const inspectPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    if (paths.length !== 1) {
      setError('PDF Compression accepts exactly one local PDF.');
      return;
    }
    setError(null);
    try {
      const [inspection] = await api.files.inspect(paths);
      if (!inspection || inspection.mimeType !== 'application/pdf') {
        setError('Choose a local file with a .pdf name and valid PDF magic.');
        return;
      }
      setSource(inspection);
      setOutputName(compressedName(inspection.displayName));
      setJob(null);
      setAudit(null);
      setProgress(null);
      setAnnouncement(`${inspection.displayName} passed local file preflight.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void api.files.onPdfDrop((paths) => {
      if (active && !busy) void inspectPaths(paths);
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, [busy]);

  const chooseSource = async () => inspectPaths(await api.dialogs.selectPdfInputs());
  const chooseDestination = async () => {
    setError(null);
    const selected = await api.dialogs.selectDestination();
    if (selected) setDestination(selected);
  };
  const removeSource = () => {
    setSource(null);
    setJob(null);
    setAudit(null);
    setProgress(null);
    setAnnouncement('Selected PDF removed.');
    requestAnimationFrame(() => selectButton.current?.focus());
  };
  const qpdf = dependencies.find((dependency) => dependency.id === 'qpdf');
  const pdfjs = dependencies.find((dependency) => dependency.id === 'pdfjs');
  const qpdfAvailable = qpdf?.status === 'available'
    && qpdf.version === '12.3.2'
    && qpdf.capabilities.includes(profile === 'balanced'
      ? 'pdf.compress-balanced'
      : 'pdf.compress-lossless');
  const balancedRendererAvailable = pdfjs?.status === 'available'
    && pdfjs.version === '6.2.108'
    && pdfjs.capabilities.includes('pdf.compress-balanced');
  const validation = !source ? 'Open one PDF.'
    : !destination ? 'Choose a destination folder.'
      : !validPdfOutputName(outputName) ? 'Enter a Windows-safe filename ending in .pdf.'
        : !qpdfAvailable ? 'The bundled qpdf 12.3.2 compression boundary must pass its local check.'
          : profile === 'balanced' && !balancedRendererAvailable
            ? 'The bundled PDF.js 6.2.108 visual-verification boundary must pass its local check.'
          : null;
  const startCompression = async () => {
    if (!source || !destination || validation) return;
    const previousOperation = balancedOperation.current;
    if (previousOperation && !previousOperation.canBeReplaced) return;
    const generation = operationGeneration.current + 1;
    operationGeneration.current = generation;
    jobId.current = null;
    pendingVisual.current.clear();
    setBusy(true);
    setError(null);
    setAudit(null);
    setProgress(null);
    setJob(null);
    if (profile === 'balanced') {
      const operation = new BalancedCompressionOperation(generation, api.jobs);
      balancedOperation.current = operation;
      operation.beginCreate();
      try {
        const creation = api.jobs.createBalanced({
          operationId: 'pdf.compress-balanced',
          inputPaths: [source.path],
          destinationDirectory: destination,
          requestedOutputName: outputName,
          settings: { profile: 'balanced-v1' },
        });
        const created = await creation;
        const reconciliation = operation.registerCreatedJob(created);
        jobId.current = created.id;
        const reconciled = await reconciliation;
        if (operation.isDisposed || !ownsCurrentBalancedOperation(operation)) return;
        setJob(reconciled ?? created);
        setAnnouncement('Balanced compression job queued with the fixed balanced-v1 profile.');
        const pending = pendingVisual.current.get(created.id);
        pendingVisual.current.clear();
        if (pending?.generation === operation.generation) {
          void runBalancedVisual(pending.session, operation);
        }
      } catch (reason) {
        if (operation.createState === 'pending') operation.failCreate();
        if (ownsCurrentBalancedOperation(operation)) {
          setBusy(false);
          setError(operationErrorMessage(reason));
        }
      }
      return;
    }
    balancedOperation.current = null;
    try {
      const created = await api.jobs.create({
        operationId: 'pdf.compress-lossless',
        inputPaths: [source.path],
        destinationDirectory: destination,
        requestedOutputName: outputName,
      });
      if (!mounted.current || generation !== operationGeneration.current) return;
      jobId.current = created.id;
      setJob(created);
      setAnnouncement('Lossless compression job queued.');
    } catch (reason) {
      if (mounted.current && generation === operationGeneration.current) {
        setBusy(false);
        setError(operationErrorMessage(reason));
      }
    }
  };
  const changeProfile = (nextProfile: 'lossless' | 'balanced') => {
    operationGeneration.current += 1;
    balancedOperation.current = null;
    jobId.current = null;
    pendingVisual.current.clear();
    setProfile(nextProfile);
    setAudit(null);
    setJob(null);
  };
  const cancel = async () => {
    if (!job) return;
    try {
      cancelVisualResources();
      const operation = balancedOperation.current;
      if (operation?.ownsJob(job.id)) {
        const snapshot = await operation.reconcileOwnedJob();
        if (ownsCurrentBalancedOperation(operation)) {
          if (snapshot) setJob(snapshot);
          if (snapshot && terminalStates.has(snapshot.state)) setBusy(false);
          setAnnouncement('Cancellation requested. Owned temporary data is being reconciled.');
        }
        return;
      }
      await api.jobs.cancel({ jobId: job.id });
      setAnnouncement('Cancellation requested. Owned temporary data is being reconciled.');
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };
  const resolveInterrupted = async () => {
    if (!job) return;
    try {
      const resolved = await api.jobs.resolveInterrupted({ jobId: job.id });
      setJob(resolved);
      setAnnouncement(`Interrupted job resolved as ${resolved.state}.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const activeError = job?.errors.at(-1);
  const cancellable = progress?.cancellable
    ?? Boolean(job && !terminalStates.has(job.state) && job.state !== 'publishing');
  const indeterminate = progress?.messageCode === 'COMPRESSING_PDF_LOSSLESSLY'
    || progress?.messageCode === 'BALANCED_SELECTING_IMAGES';
  const progressValue = progress?.completedUnits ?? job?.progress.completedUnits ?? 0;
  const progressTotal = progress?.totalUnits ?? job?.progress.totalUnits ?? 0;
  const progressPercent = progressTotal > 0
    ? Math.min(100, Math.round((progressValue / progressTotal) * 100))
    : job?.state === 'completed' ? 100 : 0;
  const afterBytes = job?.outputs[0]?.sizeBytes;
  const sizeReport = source && afterBytes != null
    ? calculateSizeReport(source.sizeBytes, afterBytes)
    : null;

  return (
    <div className="app-shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand" aria-label="Document Studio">DS</div>
        <button className="rail-button" onClick={onOpenMerge}>Merge</button>
        <button className="rail-button" onClick={onOpenViewer}>Viewer</button>
        <button className="rail-button active" aria-current="page">Optimize</button>
        <button className="rail-button" onClick={onOpenConvert}>Convert</button>
        <button className="rail-button" disabled>Settings</button>
      </aside>
      <main className="workspace">
        <header className="page-header">
          <div>
            <p className="eyebrow">OPTIMIZE · LOCAL ONLY</p>
            <h1>{profile === 'balanced' ? 'Balanced PDF Compression' : 'Lossless PDF Compression'}</h1>
            <p className="lede">{profile === 'balanced'
              ? 'Re-encode only proven-safe photographic image streams, then verify every affected page before any output can be published.'
              : 'Recompress PDF structure without intentionally reducing image quality or document content.'}</p>
          </div>
          <div className="privacy-badge"><span aria-hidden="true">●</span>{system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}</div>
        </header>
        {error && <div className="error-banner" role="alert">{error}</div>}
        <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
        <section className="optimize-layout" aria-label={`${profile === 'balanced' ? 'Balanced' : 'Lossless'} PDF Compression workspace`}>
          <article className="card optimize-source-card">
            <div className="card-heading">
              <div><p className="eyebrow">SOURCE</p><h2>Open one PDF</h2></div>
              <span className={`status-chip ${qpdfAvailable ? '' : 'unavailable'}`}>{qpdfAvailable ? 'qpdf 12.3.2 verified' : 'Engine unavailable'}</span>
            </div>
            <div className="compression-profile" role="group" aria-label="Compression profile">
              <button
                type="button"
                className={profile === 'lossless' ? 'secondary active' : 'secondary'}
                aria-pressed={profile === 'lossless'}
                disabled={busy}
                onClick={() => { changeProfile('lossless'); }}
              >Lossless</button>
              <button
                type="button"
                className={profile === 'balanced' ? 'secondary active' : 'secondary'}
                aria-pressed={profile === 'balanced'}
                disabled={busy}
                onClick={() => { changeProfile('balanced'); }}
              >Balanced</button>
            </div>
            {!source ? (
              <button ref={selectButton} type="button" className="drop-zone" onClick={chooseSource} disabled={busy}>
                <strong>Open PDF</strong><span>Choose or drop exactly one local, unencrypted PDF</span>
              </button>
            ) : (
              <div className="selection-row">
                <div><span className="field-label">Selected PDF</span><strong title={source.path}>{source.displayName}</strong><small>{formatBytes(source.sizeBytes)} · source remains immutable</small></div>
                <button type="button" className="secondary danger" onClick={removeSource} disabled={busy}>Remove</button>
              </div>
            )}
            {source && <div className="preflight-ready" role="status"><strong>Local file preflight ready</strong><span>{profile === 'balanced' ? 'qpdf will refuse encryption, signatures, recovery, malformed structure, and unsafe image resources before candidate generation.' : 'PDF structure, encryption, page count, and preservation inventory are checked by qpdf before and after recompression.'}</span></div>}
            {profile === 'lossless' ? <div className="lossless-note" role="note"><strong>What lossless means here</strong><p>Document Studio recompresses streams and object structure. It does not rasterize pages, flatten forms, or intentionally reduce image quality. The result can be smaller, unchanged, or larger.</p></div> : <div className="lossless-note" role="note"><strong>Fixed balanced-v1 profile</strong><p>Quality 82, no resampling. Only safe indirect RGB8 DCT or simple-Flate image XObjects are eligible. Text, vectors, and page structure are never rasterized, while unsupported images remain unchanged. Every affected page must pass SSIM ≥ 0.985, PSNR ≥ 36 dB, and the changed-pixel limit at 144 DPI. A file is published only when it saves both 5% and 64 KiB; no benefit creates no file.</p></div>}
            <div className="signature-warning" role="note"><strong>{profile === 'balanced' ? 'Signed documents are refused' : 'Digital signature warning'}</strong><p>{profile === 'balanced' ? 'Balanced compression stops before writing a candidate when signature or ByteRange evidence is present.' : 'PDF rewriting invalidates existing digital signatures. If this PDF is signed, its signatures will no longer validate after compression.'}</p></div>
          </article>
          <aside className="side-stack">
            <article className="card output-card">
              <p className="eyebrow">VERIFIED OUTPUT</p><h2>Save compressed PDF</h2>
              <div className="selection-row compact-selection"><div><span className="field-label">Destination</span><strong>{destination ?? 'No folder selected'}</strong><small>Existing user files are never overwritten.</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div>
              <label className="output-field"><span className="field-label">Output filename</span><input value={outputName} onChange={(event) => setOutputName(event.target.value)} disabled={busy} aria-describedby="compression-output-help" /><small id="compression-output-help">A collision receives a numbered name through the existing safe publication boundary.</small></label>
              {validation && <p className="preflight-message" role="status">{validation}</p>}
              <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={startCompression}>{profile === 'balanced' ? 'Compress with Balanced' : 'Compress Losslessly'}</button>{busy && <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancel}>{cancellable ? 'Cancel' : 'Publishing safely'}</button>}</div>
            </article>
            <article className="card job-card" aria-live="polite" aria-atomic="true">
              <div className="card-heading"><h2>Compression status</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div>
              <strong>{progress?.message ?? (job ? `Job ${job.state}` : 'Ready for local preflight')}</strong>
              {indeterminate ? <div className="indeterminate-bar" role="progressbar" aria-label="Compressing PDF" /> : <progress value={progressPercent} max={100} aria-label={`${profile === 'balanced' ? 'Balanced' : 'Lossless'} PDF Compression progress`} />}
              {busy && !cancellable && <p className="publishing-copy">Publishing safely—cancellation is no longer available.</p>}
              {job?.state === 'completed' && job.completionKind === 'no-benefit' && <NoBenefitResult resultRef={result} />}
              {job?.state === 'completed' && job.completionKind !== 'no-benefit' && sizeReport && <div ref={result} className="success-result compression-result" tabIndex={-1}><strong>Verified compressed PDF</strong><span>{job.outputs[0]?.finalPath}</span><div className="size-comparison" aria-label="Compression size comparison"><span><small>Before</small><strong>{source?.sizeBytes.toLocaleString()} bytes</strong></span><span><small>After</small><strong>{afterBytes?.toLocaleString()} bytes</strong></span><span><small>Delta</small><strong>{formatSignedBytes(sizeReport.deltaBytes)} · {formatSignedPercentage(sizeReport.percentageDelta)}</strong></span></div><p className={`size-outcome outcome-${sizeReport.outcome}`}>{sizeOutcomeLabel(sizeReport)}</p><div className="action-row"><button type="button" className="secondary compact" onClick={() => void navigator.clipboard?.writeText(job.outputs[0]?.finalPath ?? '')}>Copy saved path</button><button type="button" className="secondary compact" onClick={onOpenViewer}>Open PDF Viewer</button></div></div>}
              {audit && <div className="balanced-audit" aria-label="Balanced compression verification evidence"><strong>balanced-v1 verification</strong><span>{formatBytes(audit.sourceBytes)} source · {formatBytes(audit.candidateBytes)} candidate · {formatBytes(audit.savedBytes)} saved ({audit.savedPercent.toFixed(2)}%)</span><span>{audit.affectedPages} affected page{audit.affectedPages === 1 ? '' : 's'} · {audit.selectedImages} image{audit.selectedImages === 1 ? '' : 's'} replaced · {audit.skippedImages} skipped</span>{audit.skippedReasons.length > 0 && <small>Skipped: {audit.skippedReasons.map((entry) => `${entry.reason} (${entry.count})`).join(', ')}</small>}{audit.minimumSsim != null && <small>Minimum SSIM {audit.minimumSsim.toFixed(6)} · Minimum PSNR {audit.psnrIsInfinite ? '∞' : `${audit.minimumPsnrDb?.toFixed(3)} dB`} · maximum changed pixels {audit.maximumChangedPixels.toLocaleString()} / {audit.maximumTotalPixels.toLocaleString()}</small>}<small>Publication gate: {audit.sizeGatePassed ? 'passed both 5% and 64 KiB' : 'did not pass both 5% and 64 KiB'}</small></div>}
              {(job?.state === 'failed' || job?.state === 'cancelled' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{activeError?.title ?? `Compression ${job.state}`}</strong><span>{activeError?.detail ?? 'No unverified output was published.'}</span>{job.state === 'interrupted' && <button type="button" className="secondary compact" onClick={resolveInterrupted}>Resolve safely</button>}</div>}
            </article>
          </aside>
        </section>
      </main>
    </div>
  );
}
