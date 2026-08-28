import { useEffect, useMemo, useRef, useState } from 'react';
import type {
  DestinationGrant,
  JobRecord,
  ProgressEvent,
  TextInputMetadata,
  TextOrientation,
  TextPageSize,
  ViewerDocumentMetadata,
} from '@document-studio/contracts';
import { api, createProgressReconciler, operationErrorMessage } from './api';
import { formatBytes } from './sizeReporting';

const terminalStates = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

function validPdfOutputName(name: string): boolean {
  return name.length > 4 && name.length <= 255
    && name.toLocaleLowerCase().endsWith('.pdf')
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(name)
    && !/[. ]$/u.test(name)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(name);
}

interface TextToPdfWorkspaceProps {
  onBusyChange: (busy: boolean) => void;
  onOpenViewer: (document?: ViewerDocumentMetadata) => void;
}

export function TextToPdfWorkspace({
  onBusyChange,
  onOpenViewer,
}: TextToPdfWorkspaceProps) {
  const [source, setSource] = useState<TextInputMetadata | null>(null);
  const [destination, setDestination] = useState<DestinationGrant | null>(null);
  const [pageSize, setPageSize] = useState<TextPageSize>('a4');
  const [orientation, setOrientation] = useState<TextOrientation>('portrait');
  const [outputName, setOutputName] = useState('document.pdf');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const jobId = useRef<string | null>(null);
  const openButton = useRef<HTMLButtonElement | null>(null);
  const result = useRef<HTMLDivElement | null>(null);
  const savedPath = useRef<HTMLSpanElement | null>(null);

  useEffect(() => onBusyChange(busy), [busy, onBusyChange]);

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
            if (!active) return;
            setJob(snapshot);
            setSource(null);
            if (snapshot.state === 'completed') {
              queueMicrotask(() => result.current?.focus());
            } else {
              queueMicrotask(() => openButton.current?.focus());
            }
          }).catch((reason: unknown) => active && setError(operationErrorMessage(reason)));
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

  const validation = useMemo(() => {
    if (!source) return 'Choose exactly one local .txt file.';
    if (!destination) return 'Choose a destination folder.';
    if (!validPdfOutputName(outputName)) return 'Enter a Windows-safe filename ending in .pdf.';
    return null;
  }, [destination, outputName, source]);

  const openText = async () => {
    setError(null);
    try {
      const selected = await api.text.open();
      if (!selected) return;
      if (source) {
        await api.viewer.close({ sessionId: source.sessionId, generation: source.generation }).catch(() => undefined);
      }
      setSource(selected);
      setJob(null);
      setProgress(null);
      jobId.current = null;
      const stem = selected.displayName.replace(/\.txt$/iu, '').slice(0, 240).trim();
      setOutputName(`${stem || 'document'}.pdf`);
      setAnnouncement('TXT selected. The document body stays outside React and durable history.');
    } catch (reason) {
      setError(operationErrorMessage(reason));
      queueMicrotask(() => openButton.current?.focus());
    }
  };

  const chooseDestination = async () => {
    setError(null);
    try {
      const selected = await api.viewer.chooseDestination();
      if (!selected) return;
      if (destination) await api.viewer.revokeDestination(destination.grantId);
      setDestination(selected);
      setAnnouncement('Local destination selected through an opaque grant.');
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const start = async () => {
    if (!source || !destination || validation) return;
    setBusy(true);
    setError(null);
    setJob(null);
    setProgress(null);
    try {
      const created = await api.jobs.createTextToPdf({
        operationId: 'text.to-pdf',
        inputSessionId: source.sessionId,
        inputGeneration: source.generation,
        destinationGrantId: destination.grantId,
        requestedOutputName: outputName,
        settings: { pageSize, orientation },
      });
      jobId.current = created.id;
      setJob(created);
      await api.viewer.revokeDestination(destination.grantId).catch(() => undefined);
      setDestination(null);
      setAnnouncement('TXT-to-PDF job queued for strict native preflight.');
      void api.jobs.get({ jobId: created.id }).then((snapshot) => {
        if (snapshot.id !== jobId.current) return;
        setJob(snapshot);
        if (terminalStates.has(snapshot.state)) {
          setBusy(false);
          setSource(null);
          queueMicrotask(() => snapshot.state === 'completed'
            ? result.current?.focus()
            : openButton.current?.focus());
        }
      }).catch((reason: unknown) => setError(operationErrorMessage(reason)));
    } catch (reason) {
      setBusy(false);
      setError(operationErrorMessage(reason));
      queueMicrotask(() => openButton.current?.focus());
    }
  };

  const cancel = async () => {
    if (!job) return;
    try {
      await api.jobs.cancel({ jobId: job.id });
      setAnnouncement('Cancellation requested. Exact owned renderer data is being reconciled.');
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const openVerifiedOutput = async () => {
    if (!job || job.state !== 'completed') return;
    setError(null);
    try {
      const document = await api.text.openOutput(job.id);
      onOpenViewer(document);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const revealSavedLocation = () => {
    savedPath.current?.focus();
    setAnnouncement('The verified saved path is shown and focused below. No operating-system shell was invoked.');
  };

  const activeError = job?.errors.at(-1);
  const cancellable = progress?.cancellable
    ?? Boolean(job && !terminalStates.has(job.state) && job.state !== 'publishing');
  const completedUnits = progress?.completedUnits ?? job?.progress.completedUnits ?? 0;
  const totalUnits = progress?.totalUnits ?? job?.progress.totalUnits ?? 18;
  const progressPercent = totalUnits > 0
    ? Math.min(100, Math.round((completedUnits / totalUnits) * 100))
    : 0;
  const finalPath = job?.outputs[0]?.finalPath ?? null;

  return <>
    {error && <div className="error-banner" role="alert">{error}</div>}
    <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
    <section className="convert-layout" aria-label="TXT to PDF workspace">
      <article className="card">
        <div className="card-heading">
          <div><p className="eyebrow">STRICT UTF-8 TXT</p><h2>Private text, fixed rendering</h2></div>
          <span className="status-chip">Local · offline</span>
        </div>
        <div className="selection-row compact-selection">
          <div>
            <span className="field-label">Source TXT</span>
            <strong>{source?.displayName ?? 'No TXT selected'}</strong>
            <small>{source ? `${formatBytes(source.sizeBytes)} · retained read-only native handle` : 'Exactly one regular, non-link .txt file'}</small>
          </div>
          <button ref={openButton} type="button" className="secondary" onClick={openText} disabled={busy}>{source ? 'Replace' : 'Choose TXT'}</button>
        </div>
        <div className="page-only-notice" role="note">
          <strong>Accepted input</strong>
          <p>Strict UTF-8 only · at most 8,388,608 bytes · 100,000 logical lines · 65,536 UTF-8 bytes per normalized line. An initial UTF-8 BOM is removed; UTF-16/32, controls, bidi controls, unsupported Unicode, and unbounded shaping are rejected before WebView2 starts.</p>
        </div>
        <div className="page-only-notice" role="note">
          <strong>Fixed fonts and privacy</strong>
          <p>Packaged Noto Sans Regular supports English, Hindi (Devanagari), and Telugu with admitted punctuation. No system font, synthetic face, preview, upload, runtime download, or document text is sent to React, events, diagnostics, or durable history.</p>
        </div>
      </article>
      <aside className="side-stack">
        <article className="card output-card">
          <p className="eyebrow">FIXED PDF POLICY</p><h2>Page and destination</h2>
          <fieldset className="choice-grid two-choice">
            <legend>Page size</legend>
            <label><input type="radio" name="txt-page-size" value="a4" checked={pageSize === 'a4'} onChange={() => setPageSize('a4')} disabled={busy} />A4</label>
            <label><input type="radio" name="txt-page-size" value="letter" checked={pageSize === 'letter'} onChange={() => setPageSize('letter')} disabled={busy} />Letter</label>
          </fieldset>
          <fieldset className="choice-grid two-choice">
            <legend>Orientation</legend>
            <label><input type="radio" name="txt-orientation" value="portrait" checked={orientation === 'portrait'} onChange={() => setOrientation('portrait')} disabled={busy} />Portrait</label>
            <label><input type="radio" name="txt-orientation" value="landscape" checked={orientation === 'landscape'} onChange={() => setOrientation('landscape')} disabled={busy} />Landscape</label>
          </fieldset>
          <div className="selection-row compact-selection">
            <div><span className="field-label">Destination</span><strong>{destination?.displayName ?? 'No folder selected'}</strong><small>Opaque local grant · existing files are never overwritten</small></div>
            <button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button>
          </div>
          <label className="output-field" htmlFor="txt-output-name"><span className="field-label">Requested output filename</span><input id="txt-output-name" value={outputName} onChange={(event) => setOutputName(event.target.value)} aria-describedby="txt-output-help" disabled={busy} /><small id="txt-output-help">A destination collision receives a numbered no-overwrite name; the verified final name is shown below.</small></label>
          {validation && <p className="preflight-message" role="status">{validation}</p>}
          <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={start}>Create verified PDF</button>{busy && <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancel}>{cancellable ? 'Cancel' : 'Publishing safely'}</button>}</div>
        </article>
        <article className="card job-card">
          <div className="card-heading"><h2>TXT conversion status</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div>
          <strong>{progress?.message ?? (job ? `Job ${job.state}` : 'Ready for strict TXT preflight')}</strong>
          <small>{progress?.stage ? `Current stage: ${progress.stage}` : 'No renderer has been created.'}</small>
          <progress value={progressPercent} max={100} aria-label="TXT to PDF progress" />
          {busy && !cancellable && <p className="publishing-copy">Publishing safely—cancellation cannot remove a committed user file.</p>}
          {job?.state === 'completed' && finalPath && <div ref={result} className="success-result" tabIndex={-1}><strong>Verified TXT PDF</strong><span ref={savedPath} tabIndex={-1}>{finalPath}</span><small>{job.outputs[0]?.sizeBytes == null ? '' : formatBytes(job.outputs[0].sizeBytes)}</small><div className="action-row"><button type="button" className="secondary compact" onClick={openVerifiedOutput}>Open in Viewer</button><button type="button" className="secondary compact" onClick={revealSavedLocation}>Reveal saved location</button><button type="button" className="secondary compact" onClick={() => void navigator.clipboard?.writeText(finalPath)}>Copy saved path</button></div></div>}
          {(job?.state === 'failed' || job?.state === 'cancelled' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{activeError?.title ?? `Conversion ${job.state}`}</strong><span>{activeError?.detail ?? 'No unverified output was published.'}</span></div>}
        </article>
      </aside>
    </section>
  </>;
}
