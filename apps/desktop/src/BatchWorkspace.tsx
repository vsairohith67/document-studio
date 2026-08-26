import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import type {
  BatchPreviewRequest,
  BatchPreviewResponse,
  BatchRecord,
  FileInspection,
  SystemStatus,
} from '@document-studio/contracts';
import { api, operationErrorMessage } from './api';
import { formatBytes } from './sizeReporting';

interface BatchWorkspaceProps {
  system: SystemStatus | null;
  onOpenMerge: () => void;
  onOpenViewer: () => void;
  onOpenOptimize: () => void;
  onOpenConvert: () => void;
  onCreated?: () => void;
}

type BatchInput = FileInspection;

export function BatchWorkspace({
  system,
  onOpenMerge,
  onOpenViewer,
  onOpenOptimize,
  onOpenConvert,
  onCreated = () => undefined,
}: BatchWorkspaceProps) {
  const [inputs, setInputs] = useState<BatchInput[]>([]);
  const [destination, setDestination] = useState<string | null>(null);
  const [namingTemplate, setNamingTemplate] = useState('{stem}-compressed.pdf');
  const [preview, setPreview] = useState<BatchPreviewResponse | null>(null);
  const [batch, setBatch] = useState<BatchRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const inputListRef = useRef<HTMLOListElement>(null);
  const confirmationRef = useRef<HTMLDivElement>(null);
  const pendingInputFocus = useRef<number | 'add' | null>(null);

  useLayoutEffect(() => {
    const target = pendingInputFocus.current;
    if (target === null) return;
    pendingInputFocus.current = null;
    if (target === 'add') addButtonRef.current?.focus();
    else inputListRef.current
      ?.querySelector<HTMLButtonElement>(`[data-batch-index="${target}"]`)
      ?.focus();
  }, [inputs]);

  useLayoutEffect(() => {
    if (batch) confirmationRef.current?.focus();
  }, [batch]);

  const invalidatePreview = () => {
    setPreview(null);
    setBatch(null);
  };
  const inspectPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    if (inputs.length + paths.length > 128) {
      setError('Batch preview accepts no more than 128 PDFs.');
      return;
    }
    setError(null);
    try {
      const inspected = await api.files.inspect(paths);
      const invalid = inspected.find((file) => file.mimeType !== 'application/pdf');
      if (invalid) {
        setError(`${invalid.displayName} is not a valid local PDF.`);
        return;
      }
      const next = [
        ...inputs,
        ...inspected,
      ];
      setInputs(next);
      invalidatePreview();
      setAnnouncement(`${inspected.length} PDF${inspected.length === 1 ? '' : 's'} added in order.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };
  const chooseInputs = async () => inspectPaths(await api.dialogs.selectPdfInputs());
  const chooseDestination = async () => {
    const selected = await api.dialogs.selectDestination();
    if (!selected) return;
    setDestination(selected);
    invalidatePreview();
    setAnnouncement('Destination selected. Create a fresh preview before saving metadata.');
  };
  const removeInput = (index: number) => {
    setInputs((current) => {
      const next = current.filter((_, itemIndex) => itemIndex !== index);
      pendingInputFocus.current = next.length === 0 ? 'add' : Math.min(index, next.length - 1);
      return next;
    });
    invalidatePreview();
    setAnnouncement('PDF removed. Remaining input order was preserved.');
  };
  const request = useMemo<BatchPreviewRequest | null>(() => {
    if (inputs.length === 0 || !destination || namingTemplate.length === 0 || namingTemplate.length > 1024) {
      return null;
    }
    return {
      schemaVersion: 1,
      operationId: 'pdf.compress-lossless',
      operationVersion: '1.0.0',
      settings: {},
      inputPaths: inputs.map((input) => input.path),
      destinationDirectory: destination,
      namingTemplate,
    };
  }, [destination, inputs, namingTemplate]);
  const validation = inputs.length === 0 ? 'Add 1–128 PDFs.'
    : !destination ? 'Choose a destination folder.'
      : namingTemplate.length === 0 || namingTemplate.length > 1024
        ? 'Enter a naming template up to 1,024 characters.' : null;

  const createPreview = async () => {
    if (!request) return;
    setBusy(true);
    setError(null);
    setBatch(null);
    try {
      const result = await api.batches.preview(request);
      setPreview(result);
      setAnnouncement(`Preview ready for ${result.rows.length} ordered PDFs.`);
    } catch (reason) {
      setPreview(null);
      setError(operationErrorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  const createMetadata = async () => {
    if (!request || !preview) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.batches.create({
        ...request,
        previewSha256: preview.previewSha256,
        optimisticVersion: preview.optimisticVersion,
      });
      setBatch(created);
      setAnnouncement(`Batch metadata created for ${created.progress.totalChildren} children. No child was started.`);
      onCreated();
    } catch (reason) {
      setBatch(null);
      setPreview(null);
      setError(operationErrorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="app-shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand" aria-label="Document Studio">DS</div>
        <button className="rail-button" onClick={onOpenMerge}>Merge</button>
        <button className="rail-button" onClick={onOpenViewer}>Viewer</button>
        <button className="rail-button" onClick={onOpenOptimize}>Optimize</button>
        <button className="rail-button" onClick={onOpenConvert}>Convert</button>
        <button className="rail-button active" aria-current="page">Batch</button>
        <button className="rail-button" disabled>Settings</button>
      </aside>
      <main className="workspace">
        <header className="page-header">
          <div><p className="eyebrow">BATCH PREVIEW · LOCAL ONLY</p><h1>Review lossless compression metadata</h1><p className="lede">Fingerprint up to 128 PDFs, verify names and disk bounds, then create queued metadata without starting document work.</p></div>
          <div className="privacy-badge"><span aria-hidden="true">●</span>{system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}</div>
        </header>
        {error && <div className="error-banner" role="alert">{error}</div>}
        <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
        <section className="batch-layout" aria-label="Batch preview workspace">
          <article className="card batch-input-card">
            <div className="card-heading"><div><p className="eyebrow">ORDERED SOURCES</p><h2>Lossless PDF list</h2></div><span className="status-chip">{inputs.length} / 128</span></div>
            <button ref={addButtonRef} type="button" className="drop-zone" onClick={chooseInputs} disabled={busy}><strong>Add PDFs</strong><span>Selection order becomes child order; every source is hashed locally</span></button>
            <ol ref={inputListRef} className="batch-input-list" aria-label="Ordered batch PDFs">
              {inputs.length === 0 && <li className="empty-inputs">No PDFs added. Preview data is never logged or synchronized.</li>}
              {inputs.map((input, index) => (
                <li className="batch-input-row" key={`${input.fileIdentity}-${index}`}>
                  <span className="ordinal" aria-hidden="true">{index + 1}</span>
                  <div><strong>{input.displayName}</strong><small>{formatBytes(input.sizeBytes)}</small></div>
                  <button type="button" data-batch-index={index} className="icon-button remove" onClick={() => removeInput(index)} disabled={busy} aria-label={`Remove ${input.displayName}`}>×</button>
                </li>
              ))}
            </ol>
          </article>
          <aside className="side-stack">
            <article className="card output-card">
              <p className="eyebrow">DESTINATION</p><h2>Preview only</h2>
              <div className="selection-row compact-selection"><div><span className="field-label">Local folder</span><strong>{destination ?? 'No folder selected'}</strong><small>Its identity and collision state are rechecked at creation.</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div>
              <label className="field-stack" htmlFor="batch-naming-template"><span className="field-label">Naming template</span><input id="batch-naming-template" value={namingTemplate} onChange={(event) => { setNamingTemplate(event.target.value); invalidatePreview(); }} disabled={busy} aria-describedby="batch-naming-help" /><small id="batch-naming-help">Use {'{stem}'}, optional {'{index}'} (001, 002…), and double braces for literal braces. Default: {'{stem}-compressed.pdf'}.</small></label>
              {validation && <p className="preflight-message" role="status">{validation}</p>}
              <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={createPreview}>Create preview</button></div>
            </article>
            <article className="card batch-preview-card">
              <div className="card-heading"><h2>Canonical preview</h2>{preview && <span className="status-chip">v{preview.schemaVersion}</span>}</div>
              {!preview && <p className="preflight-message">No preview yet. Nothing has been written to job metadata.</p>}
              {preview && <>
                <dl className="batch-estimate">
                  <div><dt>Children</dt><dd>{preview.rows.length}</dd></div>
                  <div><dt>Workspace peak</dt><dd>{formatBytes(preview.diskEstimate.workspacePeakBytes)}</dd></div>
                  <div><dt>Destination total</dt><dd>{formatBytes(preview.diskEstimate.destinationTotalBytes)}</dd></div>
                  <div><dt>Canonical bytes</dt><dd>{preview.canonicalSizeBytes.toLocaleString()}</dd></div>
                </dl>
                <ol className="collision-plan" aria-label="Collision plan">
                  {preview.rows.map((item) => <li key={item.ordinal}><span>{item.sourceName}</span><strong>{item.outputName}</strong>{item.collisionIndex > 0 && <small>collision {item.collisionIndex}</small>}</li>)}
                </ol>
                <div className="lossless-note" role="note"><strong>Metadata gate only</strong><p>Creation rehashes every source, rechecks the destination, collisions and disk space, then inserts all records atomically. It does not start a worker.</p></div>
                <div className="action-row"><button type="button" className="primary" disabled={busy || Boolean(batch)} onClick={createMetadata}>Create batch metadata</button></div>
              </>}
              {batch && <div ref={confirmationRef} className="success-result" role="status" aria-label="Batch creation confirmation" tabIndex={-1}><strong>Queued metadata created</strong><span>{batch.progress.settledChildren} of {batch.progress.totalChildren} children settled</span><small>No child was started. Scheduling is outside this slice.</small></div>}
            </article>
          </aside>
        </section>
      </main>
    </div>
  );
}
