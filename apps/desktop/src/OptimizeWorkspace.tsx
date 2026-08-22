import { useEffect, useRef, useState } from 'react';
import type {
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';
import { api, createProgressReconciler, operationErrorMessage } from './api';
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
  const [source, setSource] = useState<FileInspection | null>(null);
  const [destination, setDestination] = useState<string | null>(null);
  const [outputName, setOutputName] = useState('compressed.pdf');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const selectButton = useRef<HTMLButtonElement>(null);
  const result = useRef<HTMLDivElement>(null);
  const jobId = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const reconcile = createProgressReconciler(
      (id) => api.jobs.get({ jobId: id }),
      (snapshot) => {
        if (active && snapshot.id === jobId.current) setJob(snapshot);
      },
      (event) => {
        if (!active || event.jobId !== jobId.current) return;
        setProgress(event);
        setAnnouncement(event.message);
        if (terminalStates.has(event.state)) {
          setBusy(false);
          void api.jobs.get({ jobId: event.jobId }).then((snapshot) => {
            if (active) setJob(snapshot);
          });
        }
      },
    );
    void api.jobs.onProgress((event) => {
      if (event.jobId === jobId.current) {
        void reconcile(event).catch((reason: unknown) => active && setError(operationErrorMessage(reason)));
      }
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);

  useEffect(() => {
    if (job?.state === 'completed') result.current?.focus();
  }, [job?.state]);

  const inspectPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    if (paths.length !== 1) {
      setError('Lossless Compression accepts exactly one local PDF.');
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
    setProgress(null);
    setAnnouncement('Selected PDF removed.');
    requestAnimationFrame(() => selectButton.current?.focus());
  };
  const qpdf = dependencies.find((dependency) => dependency.id === 'qpdf');
  const qpdfAvailable = qpdf?.status === 'available'
    && qpdf.version === '12.3.2'
    && qpdf.capabilities.includes('pdf.compress-lossless');
  const validation = !source ? 'Open one PDF.'
    : !destination ? 'Choose a destination folder.'
      : !validPdfOutputName(outputName) ? 'Enter a Windows-safe filename ending in .pdf.'
        : !qpdfAvailable ? 'The bundled qpdf 12.3.2 compression boundary must pass its local check.'
          : null;
  const startCompression = async () => {
    if (!source || !destination || validation) return;
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      const created = await api.jobs.create({
        operationId: 'pdf.compress-lossless',
        inputPaths: [source.path],
        destinationDirectory: destination,
        requestedOutputName: outputName,
      });
      jobId.current = created.id;
      setJob(created);
      setAnnouncement('Lossless compression job queued.');
    } catch (reason) {
      setBusy(false);
      setError(operationErrorMessage(reason));
    }
  };
  const cancel = async () => {
    if (!job) return;
    try {
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
  const indeterminate = progress?.messageCode === 'COMPRESSING_PDF_LOSSLESSLY';
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
            <h1>Lossless PDF Compression</h1>
            <p className="lede">Recompress PDF structure without intentionally reducing image quality or document content.</p>
          </div>
          <div className="privacy-badge"><span aria-hidden="true">●</span>{system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}</div>
        </header>
        {error && <div className="error-banner" role="alert">{error}</div>}
        <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
        <section className="optimize-layout" aria-label="Lossless PDF Compression workspace">
          <article className="card optimize-source-card">
            <div className="card-heading">
              <div><p className="eyebrow">SOURCE</p><h2>Open one PDF</h2></div>
              <span className={`status-chip ${qpdfAvailable ? '' : 'unavailable'}`}>{qpdfAvailable ? 'qpdf 12.3.2 verified' : 'Engine unavailable'}</span>
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
            {source && <div className="preflight-ready" role="status"><strong>Local file preflight ready</strong><span>PDF structure, encryption, page count, and preservation inventory are checked by qpdf before and after recompression.</span></div>}
            <div className="lossless-note" role="note"><strong>What lossless means here</strong><p>Document Studio recompresses streams and object structure. It does not rasterize pages, flatten forms, or intentionally reduce image quality. The result can be smaller, unchanged, or larger.</p></div>
            <div className="signature-warning" role="note"><strong>Digital signature warning</strong><p>PDF rewriting invalidates existing digital signatures. If this PDF is signed, its signatures will no longer validate after compression.</p></div>
          </article>
          <aside className="side-stack">
            <article className="card output-card">
              <p className="eyebrow">VERIFIED OUTPUT</p><h2>Save compressed PDF</h2>
              <div className="selection-row compact-selection"><div><span className="field-label">Destination</span><strong>{destination ?? 'No folder selected'}</strong><small>Existing user files are never overwritten.</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div>
              <label className="output-field"><span className="field-label">Output filename</span><input value={outputName} onChange={(event) => setOutputName(event.target.value)} disabled={busy} aria-describedby="compression-output-help" /><small id="compression-output-help">A collision receives a numbered name through the existing safe publication boundary.</small></label>
              {validation && <p className="preflight-message" role="status">{validation}</p>}
              <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={startCompression}>Compress</button>{busy && <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancel}>{cancellable ? 'Cancel' : 'Publishing safely'}</button>}</div>
            </article>
            <article className="card job-card" aria-live="polite" aria-atomic="true">
              <div className="card-heading"><h2>Compression status</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div>
              <strong>{progress?.message ?? (job ? `Job ${job.state}` : 'Ready for local preflight')}</strong>
              {indeterminate ? <div className="indeterminate-bar" role="progressbar" aria-label="Compressing PDF" /> : <progress value={progressPercent} max={100} aria-label="Lossless PDF Compression progress" />}
              {busy && !cancellable && <p className="publishing-copy">Publishing safely—cancellation is no longer available.</p>}
              {job?.state === 'completed' && sizeReport && <div ref={result} className="success-result compression-result" tabIndex={-1}><strong>Verified compressed PDF</strong><span>{job.outputs[0]?.finalPath}</span><div className="size-comparison" aria-label="Compression size comparison"><span><small>Before</small><strong>{source?.sizeBytes.toLocaleString()} bytes</strong></span><span><small>After</small><strong>{afterBytes?.toLocaleString()} bytes</strong></span><span><small>Delta</small><strong>{formatSignedBytes(sizeReport.deltaBytes)} · {formatSignedPercentage(sizeReport.percentageDelta)}</strong></span></div><p className={`size-outcome outcome-${sizeReport.outcome}`}>{sizeOutcomeLabel(sizeReport)}</p><div className="action-row"><button type="button" className="secondary compact" onClick={() => void navigator.clipboard?.writeText(job.outputs[0]?.finalPath ?? '')}>Copy saved path</button><button type="button" className="secondary compact" onClick={onOpenViewer}>Open PDF Viewer</button></div></div>}
              {(job?.state === 'failed' || job?.state === 'cancelled' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{activeError?.title ?? `Compression ${job.state}`}</strong><span>{activeError?.detail ?? 'No unverified output was published.'}</span>{job.state === 'interrupted' && <button type="button" className="secondary compact" onClick={resolveInterrupted}>Resolve safely</button>}</div>}
            </article>
          </aside>
        </section>
      </main>
    </div>
  );
}
