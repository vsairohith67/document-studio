import { useEffect, useMemo, useRef, useState } from 'react';
import type {
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  JobWarning,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';
import { api, createProgressReconciler, operationErrorMessage } from './api';
import { NoBenefitResult } from './JobCompletionOutcome';
import { formatBytes } from './sizeReporting';
import { PdfToImagesWorkspace } from './PdfToImagesWorkspace';

const terminalStates = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const acceptedImageTypes = new Set(['image/jpeg', 'image/png', 'image/webp']);
const maximumImages = 128;

type SelectedImage = FileInspection & { selectionId: string };

function validPdfOutputName(name: string): boolean {
  return name.length > 4 && name.length <= 255
    && name.toLocaleLowerCase().endsWith('.pdf')
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(name)
    && !/[. ]$/u.test(name)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(name);
}

interface ConvertWorkspaceProps {
  system: SystemStatus | null;
  dependencies: DependencyDiagnostic[];
  onOpenMerge: () => void;
  onOpenViewer: () => void;
  onOpenOptimize: () => void;
  onOpenBatch?: () => void;
}

export function ConvertWorkspace({
  system,
  dependencies,
  onOpenMerge,
  onOpenViewer,
  onOpenOptimize,
  onOpenBatch = () => undefined,
}: ConvertWorkspaceProps) {
  const [images, setImages] = useState<SelectedImage[]>([]);
  const [destination, setDestination] = useState<string | null>(null);
  const [outputName, setOutputName] = useState('images.pdf');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [warnings, setWarnings] = useState<JobWarning[]>([]);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const [direction, setDirection] = useState<'images-to-pdf' | 'pdf-to-images'>('images-to-pdf');
  const nextSelectionId = useRef(0);
  const jobId = useRef<string | null>(null);
  const busyRef = useRef(false);
  const imageCountRef = useRef(0);

  useEffect(() => { busyRef.current = busy; }, [busy]);
  useEffect(() => { imageCountRef.current = images.length; }, [images.length]);

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
          void Promise.all([
            api.jobs.get({ jobId: event.jobId }),
            api.jobs.warnings({ jobId: event.jobId }),
          ]).then(([snapshot, terminalWarnings]) => {
            if (active) {
              setJob(snapshot);
              setWarnings(terminalWarnings);
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

  const inspectPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    setError(null);
    const remaining = maximumImages - imageCountRef.current;
    if (remaining <= 0 || paths.length > remaining) {
      setError('Images to PDF accepts no more than 128 selected files.');
      return;
    }
    try {
      const inspected = await api.files.inspect(paths);
      const unsupported = inspected.find((file) => !acceptedImageTypes.has(file.mimeType));
      if (unsupported) {
        setError(`${unsupported.displayName} is not valid JPEG, PNG, or WebP content.`);
        return;
      }
      const selected = inspected.map((image) => ({
        ...image,
        selectionId: `image-selection-${++nextSelectionId.current}`,
      }));
      setImages((current) => [...current, ...selected]);
      setJob(null);
      setWarnings([]);
      setProgress(null);
      setAnnouncement(`${selected.length} image${selected.length === 1 ? '' : 's'} added in selected order.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void api.files.onPdfDrop((paths) => {
      if (active && !busyRef.current) void inspectPaths(paths);
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);

  const move = (selectionId: string, target: number) => {
    if (busy) return;
    setImages((current) => {
      const source = current.findIndex((image) => image.selectionId === selectionId);
      if (source < 0 || target < 0 || target >= current.length || source === target) return current;
      const next = [...current];
      const [item] = next.splice(source, 1);
      next.splice(target, 0, item);
      setAnnouncement(`${item.displayName} moved to position ${target + 1}.`);
      return next;
    });
  };
  const remove = (selectionId: string) => {
    if (busy) return;
    setImages((current) => current.filter((image) => image.selectionId !== selectionId));
    setAnnouncement('Image removed from the conversion order.');
  };
  const chooseDestination = async () => {
    const selected = await api.dialogs.selectDestination();
    if (selected) setDestination(selected);
  };

  const qpdf = dependencies.find((dependency) => dependency.id === 'qpdf');
  const core = dependencies.find((dependency) => dependency.id === 'document-studio-core');
  const writerAvailable = qpdf?.status === 'available'
    && qpdf.version === '12.3.2'
    && qpdf.capabilities.includes('image.to-pdf')
    && core?.status === 'available'
    && core.capabilities.includes('image.to-pdf');
  const duplicateIdentities = useMemo(() => {
    const counts = new Map<string, number>();
    for (const image of images) counts.set(image.fileIdentity, (counts.get(image.fileIdentity) ?? 0) + 1);
    return counts;
  }, [images]);
  const validation = images.length === 0 ? 'Add at least one JPEG, PNG, or WebP image.'
    : images.length > maximumImages ? 'Remove images until no more than 128 remain.'
      : !destination ? 'Choose a destination folder.'
        : !validPdfOutputName(outputName) ? 'Enter a Windows-safe filename ending in .pdf.'
          : !writerAvailable ? 'The built-in writer and qpdf 12.3.2 verifier must pass local checks.'
            : null;
  const start = async () => {
    if (!destination || validation || images.length === 0) return;
    setBusy(true);
    setError(null);
    setProgress(null);
    setWarnings([]);
    try {
      const created = await api.jobs.create({
        operationId: 'image.to-pdf',
        inputPaths: images.map((image) => image.path) as [string, ...string[]],
        destinationDirectory: destination,
        requestedOutputName: outputName,
      });
      jobId.current = created.id;
      setJob(created);
      setAnnouncement('Image-to-PDF job queued.');
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

  const activeError = job?.errors.at(-1);
  const cancellable = progress?.cancellable
    ?? Boolean(job && !terminalStates.has(job.state) && job.state !== 'publishing');
  const completedUnits = progress?.completedUnits ?? job?.progress.completedUnits ?? 0;
  const totalUnits = progress?.totalUnits ?? job?.progress.totalUnits ?? images.length;
  const progressPercent = totalUnits > 0
    ? Math.min(100, Math.round((completedUnits / totalUnits) * 100))
    : 0;

  return (
    <div className="app-shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand" aria-label="Document Studio">DS</div>
        <button className="rail-button" onClick={onOpenMerge}>Merge</button>
        <button className="rail-button" onClick={onOpenViewer}>Viewer</button>
        <button className="rail-button" onClick={onOpenOptimize}>Optimize</button>
        <button className="rail-button active" aria-current="page">Convert</button>
        <button className="rail-button" onClick={onOpenBatch}>Batch</button>
        <button className="rail-button" disabled>Settings</button>
      </aside>
      <main className="workspace">
        <header className="page-header">
          <div><p className="eyebrow">CONVERT · LOCAL ONLY</p><h1>Images and PDF conversion</h1><p className="lede">Create a verified PDF from ordered local images without uploading files or replacing an existing output.</p></div>
          <div className="privacy-badge"><span aria-hidden="true">●</span>{system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}</div>
        </header>
        <div className="conversion-tabs" role="tablist" aria-label="Conversion direction">
          <button type="button" role="tab" aria-selected={direction === 'images-to-pdf'} className={direction === 'images-to-pdf' ? 'active' : ''} onClick={() => setDirection('images-to-pdf')} disabled={busy}>Images to PDF</button>
          <button type="button" role="tab" aria-selected={direction === 'pdf-to-images'} className={direction === 'pdf-to-images' ? 'active' : ''} onClick={() => setDirection('pdf-to-images')} disabled={busy}>PDF to images</button>
        </div>
        {direction === 'pdf-to-images' ? <PdfToImagesWorkspace /> : <>
        {error && <div className="error-banner" role="alert">{error}</div>}
        <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
        <section className="convert-layout" aria-label="Images to PDF workspace">
          <article className="card">
            <div className="card-heading"><div><p className="eyebrow">ORDERED IMAGES</p><h2>One image per page</h2></div><span className={`status-chip ${writerAvailable ? '' : 'unavailable'}`}>{writerAvailable ? 'Writer verified' : 'Writer unavailable'}</span></div>
            <button type="button" className="drop-zone" disabled={busy} onClick={() => void api.dialogs.selectImageInputs().then(inspectPaths)}><strong>Add images</strong><span>JPEG, PNG, or WebP content · 1–128 files · selection order is page order</span></button>
            <ol className="merge-list" aria-label="Images in PDF page order">
              {images.length === 0 && <li className="empty-inputs">No images added. Source files remain in their original locations.</li>}
              {images.map((image, index) => <li className="merge-row" key={image.selectionId} tabIndex={0} onKeyDown={(event) => {
                if (event.altKey && event.key === 'ArrowUp') { event.preventDefault(); move(image.selectionId, index - 1); }
                if (event.altKey && event.key === 'ArrowDown') { event.preventDefault(); move(image.selectionId, index + 1); }
                if (event.key === 'Delete') { event.preventDefault(); remove(image.selectionId); }
              }}><span className="ordinal" aria-hidden="true">{index + 1}</span><div className="file-details"><strong title={image.path}>{image.displayName}</strong><small>{image.mimeType.replace('image/', '').toUpperCase()} · {formatBytes(image.sizeBytes)}</small></div>{(duplicateIdentities.get(image.fileIdentity) ?? 0) > 1 && <span className="duplicate-chip">Repeated</span>}<div className="row-actions"><button type="button" className="icon-button" disabled={busy || index === 0} onClick={() => move(image.selectionId, index - 1)} aria-label={`Move ${image.displayName} up`}>↑</button><button type="button" className="icon-button" disabled={busy || index === images.length - 1} onClick={() => move(image.selectionId, index + 1)} aria-label={`Move ${image.displayName} down`}>↓</button><button type="button" className="icon-button remove" disabled={busy} onClick={() => remove(image.selectionId)} aria-label={`Remove ${image.displayName}`}>×</button></div></li>)}
            </ol>
            <div className="page-only-notice" role="note"><strong>Fixed v1 policy</strong><p>Each oriented pixel maps to one PDF point. Alpha is preserved with a soft mask. Embedded ICC profiles are not retained; decoded pixel values use DeviceRGB and create a sanitized job warning. No OCR, metadata transfer, optimization preset, or remote processing is used.</p></div>
          </article>
          <aside className="side-stack">
            <article className="card output-card"><p className="eyebrow">VERIFIED OUTPUT</p><h2>Save image PDF</h2><div className="selection-row compact-selection"><div><span className="field-label">Destination</span><strong>{destination ?? 'No folder selected'}</strong><small>Existing files are never overwritten.</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div><label className="output-field"><span className="field-label">Output filename</span><input value={outputName} onChange={(event) => setOutputName(event.target.value)} disabled={busy} /><small>Collisions receive a numbered name through the durable publication boundary.</small></label>{validation && <p className="preflight-message" role="status">{validation}</p>}<div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={start}>Create PDF</button>{busy && <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancel}>{cancellable ? 'Cancel' : 'Publishing safely'}</button>}</div></article>
            <article className="card job-card" aria-live="polite" aria-atomic="true"><div className="card-heading"><h2>Conversion status</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div><strong>{progress?.message ?? (job ? `Job ${job.state}` : 'Ready for local image preflight')}</strong><progress value={progressPercent} max={100} aria-label="Images to PDF progress" />{busy && !cancellable && <p className="publishing-copy">Publishing safely—cancellation is no longer available.</p>}{job?.state === 'completed' && (job.completionKind === 'no-benefit' ? <NoBenefitResult /> : <div className="success-result"><strong>Verified image PDF</strong><span>{job.outputs[0]?.finalPath}</span><small>{job.outputs[0]?.sizeBytes == null ? '' : formatBytes(job.outputs[0].sizeBytes)}</small><button type="button" className="secondary compact" onClick={() => void navigator.clipboard?.writeText(job.outputs[0]?.finalPath ?? '')}>Copy path</button></div>)}{warnings.length > 0 && <div className="job-warnings" role="status"><strong>Conversion warnings</strong><ul>{warnings.map((warning) => <li key={`${warning.code}-${warning.inputIndex ?? 'job'}-${warning.createdAt}`}>{warning.sanitizedDetail}</li>)}</ul></div>}{(job?.state === 'failed' || job?.state === 'cancelled' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{activeError?.title ?? `Conversion ${job.state}`}</strong><span>{activeError?.detail ?? 'No unverified output was published.'}</span></div>}</article>
          </aside>
        </section>
        </>}
      </main>
    </div>
  );
}
