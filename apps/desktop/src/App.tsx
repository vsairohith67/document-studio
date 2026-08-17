import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { DependencyDiagnostic, FileInspection, JobRecord, ProgressEvent, SystemStatus } from '@document-studio/contracts';
import { api, createProgressReconciler, operationErrorMessage } from './api';

const terminalStates = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const maximumInputs = 128;

type SelectedPdf = FileInspection & { selectionId: string };
type RowFocusControl = 'row' | 'move-up' | 'move-down' | 'remove';
type PendingFocus = { kind: 'add' } | { kind: 'row'; selectionId: string; control: RowFocusControl };

function shortPath(path: string | null | undefined): string {
  if (!path) return 'Not available';
  return path.replaceAll('\\', '/').split('/').filter(Boolean).at(-1) ?? path;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function validPdfOutputName(name: string): boolean {
  return name.length > 4 && name.length <= 255
    && name.toLocaleLowerCase().endsWith('.pdf')
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(name)
    && !/[. ]$/u.test(name)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(name);
}

export default function App() {
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [inputs, setInputs] = useState<SelectedPdf[]>([]);
  const [destination, setDestination] = useState<string | null>(null);
  const [outputName, setOutputName] = useState('merged.pdf');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [history, setHistory] = useState<JobRecord[]>([]);
  const [dependencies, setDependencies] = useState<DependencyDiagnostic[]>([]);
  const [retentionDays, setRetentionDays] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const [busy, setBusy] = useState(false);
  const draggedSelectionId = useRef<string | null>(null);
  const busyRef = useRef(false);
  const inputCountRef = useRef(0);
  const nextSelectionId = useRef(0);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const mergeListRef = useRef<HTMLOListElement>(null);
  const pendingFocus = useRef<PendingFocus | null>(null);

  useEffect(() => { busyRef.current = busy; }, [busy]);
  useEffect(() => { inputCountRef.current = inputs.length; }, [inputs.length]);

  const refreshHistory = async () => setHistory(await api.history.list({ limit: 8 }));

  const addPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    setError(null);
    try {
      const remaining = maximumInputs - inputCountRef.current;
      if (remaining <= 0 || paths.length > remaining) {
        setError('PDF Merge accepts no more than 128 files.');
        return;
      }
      const inspected = await api.files.inspect(paths);
      const invalid = inspected.find((file) => file.mimeType !== 'application/pdf');
      if (invalid) {
        setError(`${invalid.displayName} is not a valid local PDF.`);
        return;
      }
      const selected = inspected.map((input) => ({
        ...input,
        selectionId: `pdf-selection-${++nextSelectionId.current}`,
      }));
      setInputs((current) => [...current, ...selected]);
      setAnnouncement(`${inspected.length} PDF${inspected.length === 1 ? '' : 's'} added.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  useEffect(() => {
    let active = true;
    let unlistenProgress: (() => void) | undefined;
    let unlistenDrop: (() => void) | undefined;
    const reconciler = createProgressReconciler(
      (jobId) => api.jobs.get({ jobId }),
      (snapshot) => active && setJob(snapshot),
      (event) => {
        if (!active) return;
        setProgress(event);
        if (terminalStates.has(event.state)) {
          void api.jobs.get({ jobId: event.jobId }).then((snapshot) => active && setJob(snapshot));
          void refreshHistory();
          setBusy(false);
        }
      },
    );
    void Promise.all([
      api.system.status(), api.dependencies.scan(), api.history.list({ limit: 8 }),
      api.settings.get({ scope: 'application', key: 'history.retention_days' }),
    ]).then(([status, dependencyStatus, jobHistory, retention]) => {
      if (!active) return;
      setSystem(status);
      setDependencies(dependencyStatus);
      setHistory(jobHistory);
      setRetentionDays(typeof retention?.value === 'number' ? retention.value : null);
    }).catch((reason: unknown) => active && setError(operationErrorMessage(reason)));
    void api.jobs.onProgress((event) => {
      void reconciler(event).catch((reason: unknown) => active && setError(operationErrorMessage(reason)));
    }).then((stop) => { if (active) unlistenProgress = stop; else stop(); });
    void api.files.onPdfDrop((paths) => {
      if (active && !busyRef.current) void addPaths(paths);
    }).then((stop) => { if (active) unlistenDrop = stop; else stop(); });
    return () => { active = false; unlistenProgress?.(); unlistenDrop?.(); };
  }, []);

  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    pendingFocus.current = null;
    if (target.kind === 'add') {
      addButtonRef.current?.focus();
      return;
    }
    const row = mergeListRef.current?.querySelector<HTMLElement>(`[data-selection-id="${target.selectionId}"]`);
    if (!row) return;
    const control = target.control === 'row'
      ? row
      : row.querySelector<HTMLButtonElement>(`[data-focus-control="${target.control}"]`);
    if (control instanceof HTMLButtonElement && control.disabled) row.focus();
    else control?.focus();
  }, [inputs]);

  const chooseInputs = async () => addPaths(await api.dialogs.selectPdfInputs());
  const chooseDestination = async () => {
    setError(null);
    const path = await api.dialogs.selectDestination();
    if (path) setDestination(path);
  };
  const moveInput = (selectionId: string, to: number, control: RowFocusControl = 'row') => {
    if (busy) return;
    setInputs((current) => {
      const from = current.findIndex((input) => input.selectionId === selectionId);
      if (from < 0 || to < 0 || to >= current.length || from === to) return current;
      const next = [...current];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      pendingFocus.current = { kind: 'row', selectionId, control };
      setAnnouncement(`${moved.displayName} moved to position ${to + 1}.`);
      return next;
    });
  };
  const removeInput = (selectionId: string) => {
    if (busy) return;
    setInputs((current) => {
      const index = current.findIndex((input) => input.selectionId === selectionId);
      if (index < 0) return current;
      const removed = current[index];
      const next = current.filter((input) => input.selectionId !== selectionId);
      const focusTarget = next[index] ?? next[index - 1];
      pendingFocus.current = focusTarget
        ? { kind: 'row', selectionId: focusTarget.selectionId, control: 'row' }
        : { kind: 'add' };
      setAnnouncement(`${removed.displayName} removed.`);
      return next;
    });
  };
  const startMerge = async () => {
    if (inputs.length < 2 || !destination || !validPdfOutputName(outputName)) return;
    setBusy(true); setError(null); setProgress(null);
    try {
      const paths = inputs.map((input) => input.path) as [string, string, ...string[]];
      setJob(await api.jobs.create({ operationId: 'pdf.merge', inputPaths: paths, destinationDirectory: destination, requestedOutputName: outputName }));
    } catch (reason) { setBusy(false); setError(operationErrorMessage(reason)); }
  };
  const cancelMerge = async () => {
    if (!job) return;
    try { await api.jobs.cancel({ jobId: job.id }); setAnnouncement('Cancellation requested. Owned temporary data is being reconciled.'); }
    catch (reason) { setError(operationErrorMessage(reason)); }
  };
  const resolveInterruptedJob = async (jobId: string) => {
    setError(null);
    try { await api.jobs.resolveInterrupted({ jobId }); await refreshHistory(); }
    catch (reason) { setError(operationErrorMessage(reason)); }
  };

  const qpdf = dependencies.find((dependency) => dependency.id === 'qpdf');
  const qpdfAvailable = qpdf?.status === 'available' && qpdf.version === '12.3.2';
  const duplicateIdentities = useMemo(() => {
    const counts = new Map<string, number>();
    for (const input of inputs) counts.set(input.fileIdentity, (counts.get(input.fileIdentity) ?? 0) + 1);
    return counts;
  }, [inputs]);
  const validation = inputs.length < 2 ? 'Add at least two PDFs.'
    : inputs.length > maximumInputs ? 'Remove PDFs until no more than 128 remain.'
      : !destination ? 'Choose a destination folder.'
        : !validPdfOutputName(outputName) ? 'Enter a Windows-safe filename ending in .pdf.'
          : !qpdfAvailable ? 'The bundled qpdf 12.3.2 engine must pass its local sandbox check.' : null;
  const cancellable = progress?.cancellable ?? Boolean(job && !terminalStates.has(job.state) && job.state !== 'publishing');
  const isIndeterminate = progress?.messageCode === 'MERGING_PDFS';
  const progressValue = progress?.completedUnits ?? job?.progress.completedUnits ?? 0;
  const progressTotal = progress?.totalUnits ?? job?.progress.totalUnits ?? 0;
  const progressPercent = progressTotal > 0 ? Math.min(100, Math.round((progressValue / progressTotal) * 100)) : job?.state === 'completed' ? 100 : 0;
  const activeError = job?.errors.at(-1);

  return (
    <div className="app-shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand" aria-label="Document Studio">DS</div>
        <button className="rail-button active" aria-current="page">Merge</button>
        <button className="rail-button" disabled>Viewer</button><button className="rail-button" disabled>Tools</button><button className="rail-button" disabled>Settings</button>
      </aside>
      <main className="workspace">
        <header className="page-header">
          <div><p className="eyebrow">PDF TOOLS · LOCAL ONLY</p><h1>Merge PDFs in the order you choose</h1><p className="lede">Build one verified PDF without uploading documents or replacing an existing file.</p></div>
          <div className="privacy-badge"><span aria-hidden="true">●</span>{system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}</div>
        </header>
        {error && <div className="error-banner" role="alert">{error}</div>}
        <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
        <section className="merge-layout" aria-label="PDF Merge workspace">
          <article className="card merge-card">
            <div className="card-heading"><div><p className="eyebrow">ORDERED INPUTS</p><h2>PDF merge list</h2></div><span className={`status-chip ${qpdfAvailable ? '' : 'unavailable'}`}>{qpdfAvailable ? 'qpdf 12.3.2 verified' : 'Engine unavailable'}</span></div>
            <button ref={addButtonRef} type="button" className="drop-zone" onClick={chooseInputs} disabled={busy}><strong>Add PDFs</strong><span>Choose files or drop local PDFs anywhere in this window</span></button>
            <ol ref={mergeListRef} className="merge-list" aria-label="PDFs in merge order">
              {inputs.length === 0 && <li className="empty-inputs">No PDFs added yet. The source files will remain in their current locations.</li>}
              {inputs.map((input, index) => (
                <li className="merge-row" key={input.selectionId} data-selection-id={input.selectionId} draggable={!busy} tabIndex={0} aria-label={`${index + 1}. ${input.displayName}`}
                  onDragStart={() => { draggedSelectionId.current = input.selectionId; }} onDragOver={(event) => event.preventDefault()}
                  onDrop={() => { if (draggedSelectionId.current != null) moveInput(draggedSelectionId.current, index); draggedSelectionId.current = null; }}
                  onKeyDown={(event) => {
                    if (event.altKey && event.key === 'ArrowUp') { event.preventDefault(); moveInput(input.selectionId, index - 1); }
                    else if (event.altKey && event.key === 'ArrowDown') { event.preventDefault(); moveInput(input.selectionId, index + 1); }
                    else if (event.key === 'Delete') { event.preventDefault(); removeInput(input.selectionId); }
                  }}>
                  <span className="ordinal" aria-hidden="true">{index + 1}</span>
                  <div className="file-details"><strong title={input.path}>{input.displayName}</strong><small>{formatBytes(input.sizeBytes)} · Modified {new Date(input.modifiedAt).toLocaleDateString()}</small></div>
                  {duplicateIdentities.get(input.fileIdentity)! > 1 && <span className="duplicate-chip">Repeated</span>}
                  <div className="row-actions">
                    <button type="button" className="icon-button" data-focus-control="move-up" disabled={busy || index === 0} onClick={() => moveInput(input.selectionId, index - 1, 'move-up')} aria-label={`Move ${input.displayName} up`}>↑</button>
                    <button type="button" className="icon-button" data-focus-control="move-down" disabled={busy || index === inputs.length - 1} onClick={() => moveInput(input.selectionId, index + 1, 'move-down')} aria-label={`Move ${input.displayName} down`}>↓</button>
                    <button type="button" className="icon-button remove" data-focus-control="remove" disabled={busy} onClick={() => removeInput(input.selectionId)} aria-label={`Remove ${input.displayName}`}>×</button>
                  </div>
                </li>
              ))}
            </ol>
            <div className="page-only-notice" role="note"><strong>Page-only merge</strong><p>Document metadata, bookmarks, attachments, interactive forms and signatures are unsupported and are not preserved as supported features. Existing digital signatures will not remain valid.</p></div>
          </article>
          <aside className="side-stack">
            <article className="card output-card">
              <p className="eyebrow">OUTPUT</p><h2>Save merged PDF</h2>
              <div className="selection-row compact-selection"><div><span className="field-label">Destination</span><strong>{destination ? shortPath(destination) : 'No folder selected'}</strong><small>{destination ?? 'Choose an existing local folder.'}</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div>
              <label className="output-field"><span className="field-label">Output filename</span><input value={outputName} onChange={(event) => setOutputName(event.target.value)} disabled={busy} aria-describedby="output-help" /><small id="output-help">Existing files are never overwritten; a numbered name is chosen automatically.</small></label>
              {validation && <p className="preflight-message" role="status">{validation}</p>}
              <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={startMerge}>Merge PDFs</button>{busy && <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancelMerge}>{cancellable ? 'Cancel' : 'Publishing safely'}</button>}</div>
            </article>
            <article className="card job-card" aria-live="polite" aria-atomic="true">
              <div className="card-heading"><h2>Merge status</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div>
              <strong>{progress?.message ?? (job ? `Job ${job.state}` : 'Ready for a local merge')}</strong>
              {isIndeterminate ? <div className="indeterminate-bar" role="progressbar" aria-label="Merging PDFs" /> : <progress value={progressPercent} max={100} aria-label="PDF Merge progress" />}
              {busy && !cancellable && <p className="publishing-copy">Publishing safely—cancellation is no longer available.</p>}
              {job?.state === 'completed' && <div className="success-result"><strong>Verified merged PDF</strong><span>{job.outputs[0]?.finalPath}</span><small>{job.outputs[0]?.sizeBytes == null ? '' : formatBytes(job.outputs[0].sizeBytes)}</small><button type="button" className="secondary compact" onClick={() => void navigator.clipboard?.writeText(job.outputs[0]?.finalPath ?? '')}>Copy path</button></div>}
              {(job?.state === 'failed' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{activeError?.title ?? 'The merge did not finish'}</strong><span>{activeError?.detail ?? 'No unverified output was published.'}</span></div>}
            </article>
            <article className="card placeholder-card" aria-labelledby="viewer-heading"><p className="eyebrow">G03 PLACEHOLDER</p><h2 id="viewer-heading">PDF viewer</h2><p>Viewing, thumbnails and page-level tools are not part of PDF Merge.</p><button type="button" className="secondary" disabled>Viewer unavailable</button></article>
          </aside>
        </section>
        <section className="history-section" aria-labelledby="history-heading">
          <div className="section-heading"><div><p className="eyebrow">METADATA ONLY</p><h2 id="history-heading">Recent jobs</h2></div><span>{retentionDays == null ? 'Loading retention policy' : `${retentionDays}-day metadata retention`}</span></div>
          <div className="history-list">
            {history.length === 0 && <p className="empty-state">Completed, failed, cancelled and interrupted jobs will appear here.</p>}
            {history.map((item) => {
              const legacyCleanupUnproven = item.errors.some((itemError) => itemError.code === 'LEGACY_CLEANUP_UNPROVEN');
              return <article className="history-row" key={item.id}><div><strong>{item.operationId === 'pdf.merge' ? `${item.inputs.length} PDFs` : shortPath(item.inputs[0]?.displayName)}</strong><small>{item.operationId} · {new Date(item.updatedAt).toLocaleString()}</small>{legacyCleanupUnproven && <small className="cleanup-warning" role="status">Legacy destination cleanup is unproven. Inspect the destination manually; history is preserved.</small>}</div><span className={`job-state state-${item.state}`}>{item.state}</span><span>{item.outputs[0]?.sizeBytes == null ? '—' : formatBytes(item.outputs[0].sizeBytes)}</span>{item.state === 'interrupted' && !legacyCleanupUnproven && <button type="button" className="secondary compact" onClick={() => void resolveInterruptedJob(item.id)}>Resolve safely</button>}</article>;
            })}
          </div>
        </section>
      </main>
    </div>
  );
}
