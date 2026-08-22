import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type {
  DestinationGrant,
  JobRecord,
  PdfImageFormat,
  ViewerDocumentMetadata,
} from '@document-studio/contracts';
import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { api, operationErrorMessage } from './api';
import { PageThumbnail } from './viewer/PageSurface';
import {
  loadPdfSession,
  type LoadedPdfSession,
  type PdfLoadingResources,
} from './viewer/pdfSession';
import {
  PDF_TO_IMAGES_MAX_OUTPUTS,
  planPdfImagePages,
  renderPdfImageJob,
} from './viewer/pdfToImages';
import { PdfToImagesOperation } from './viewer/pdfToImagesLifecycle';

const terminalStates = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

function outputStem(displayName: string): string {
  return displayName
    .replace(/\.pdf$/iu, '')
    .replace(/[^\p{L}\p{N}._ -]+/gu, '-')
    .replace(/[. ]+$/u, '')
    .slice(0, 80) || 'document';
}

function validOutputStem(value: string): boolean {
  return value.length >= 1 && value.length <= 96
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(value)
    && !/[. ]$/u.test(value)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/iu.test(value);
}

function extension(format: PdfImageFormat): string {
  return format === 'jpeg' ? 'jpg' : format;
}

function readableError(reason: unknown): string {
  if (reason instanceof Error && reason.message) return reason.message;
  return operationErrorMessage(reason);
}

export function PdfToImagesWorkspace() {
  const [metadata, setMetadata] = useState<ViewerDocumentMetadata | null>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedOrder, setSelectedOrder] = useState<number[]>([]);
  const [selectionAnchor, setSelectionAnchor] = useState<number | null>(null);
  const [focusedPage, setFocusedPage] = useState(0);
  const [destination, setDestination] = useState<DestinationGrant | null>(null);
  const [format, setFormat] = useState<PdfImageFormat>('png');
  const [dpi, setDpi] = useState<72 | 150 | 300>(150);
  const [stem, setStem] = useState('document');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const resourceRef = useRef<LoadedPdfSession | PdfLoadingResources | null>(null);
  const metadataRef = useRef<ViewerDocumentMetadata | null>(null);
  const destinationRef = useRef<DestinationGrant | null>(null);
  const renderAbortRef = useRef<AbortController | null>(null);
  const renderTaskRef = useRef<RenderTask | null>(null);
  const openButtonRef = useRef<HTMLButtonElement>(null);
  const thumbnailScrollRef = useRef<HTMLDivElement>(null);
  const jobIdRef = useRef<string | null>(null);
  const operationRef = useRef<PdfToImagesOperation | null>(null);

  const thumbnailVirtualizer = useVirtualizer({
    count: pdf?.numPages ?? 0,
    getScrollElement: () => thumbnailScrollRef.current,
    estimateSize: () => 166,
    overscan: 3,
  });

  const closeDocument = useCallback(async (requireReconciliation = false) => {
    const resource = resourceRef.current;
    const opened = metadataRef.current;
    let cleanupFailed = false;
    if (resource) {
      try {
        await resource.close();
        if (resourceRef.current === resource) resourceRef.current = null;
      } catch {
        cleanupFailed = true;
        if (!requireReconciliation && resourceRef.current === resource) resourceRef.current = null;
      }
    }
    if (opened) {
      try {
        await api.viewer.close({ sessionId: opened.sessionId, generation: opened.generation });
        if (metadataRef.current === opened) metadataRef.current = null;
      } catch {
        cleanupFailed = true;
        if (!requireReconciliation && metadataRef.current === opened) metadataRef.current = null;
      }
    }
    setPdf(null);
    setMetadata(null);
    setSelectedOrder([]);
    setSelectionAnchor(null);
    setFocusedPage(0);
    if (requireReconciliation && cleanupFailed) {
      throw new Error('The local PDF source session could not be fully reconciled.');
    }
  }, []);

  const releaseOperation = useCallback((operation: PdfToImagesOperation) => {
    if (operationRef.current !== operation) return;
    operationRef.current = null;
    jobIdRef.current = null;
    renderTaskRef.current = null;
    renderAbortRef.current = null;
    setBusy(false);
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void api.jobs.onProgress((event) => {
      if (!active || event.jobId !== jobIdRef.current) return;
      setAnnouncement(event.message);
      if (terminalStates.has(event.state)) {
        void api.jobs.get({ jobId: event.jobId }).then((snapshot) => {
          if (active) setJob(snapshot);
        }).catch(() => undefined);
      }
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);

  useEffect(() => () => {
    const operation = operationRef.current;
    void operation?.requestCancellation().catch(() => undefined);
    renderTaskRef.current?.cancel();
    const destinationGrant = destinationRef.current;
    if (destinationGrant) {
      void api.viewer.revokeDestination(destinationGrant.grantId).catch(() => undefined);
    }
    const cleanup = operation
      ? operation.startFrontendCleanup(() => closeDocument(true))
      : closeDocument();
    void cleanup.catch(() => undefined);
  }, [closeDocument]);

  const openPdf = async () => {
    setLoading(true);
    setError(null);
    setJob(null);
    jobIdRef.current = null;
    await closeDocument();
    const controller = new AbortController();
    let encrypted = false;
    try {
      const opened = await api.viewer.open();
      if (!opened) return;
      metadataRef.current = opened;
      setMetadata(opened);
      const loaded = await loadPdfSession(
        opened,
        () => {
          encrypted = true;
          controller.abort();
        },
        (reason) => {
          setError(readableError(reason));
          controller.abort();
        },
        controller.signal,
        (resources) => { resourceRef.current = resources; },
      );
      resourceRef.current = loaded;
      setPdf(loaded.document);
      setStem(outputStem(opened.displayName));
      setSelectedOrder([0]);
      setFocusedPage(0);
      setAnnouncement(`${opened.displayName} opened. Page 1 selected for image output.`);
    } catch (reason) {
      setError(encrypted
        ? 'Encrypted PDFs are not supported by PDF-to-images v1.'
        : readableError(reason));
      await closeDocument();
    } finally {
      setLoading(false);
    }
  };

  const chooseDestination = async () => {
    setError(null);
    const selected = await api.viewer.chooseDestination();
    if (!selected) return;
    if (destinationRef.current) {
      await api.viewer.revokeDestination(destinationRef.current.grantId).catch(() => undefined);
    }
    destinationRef.current = selected;
    setDestination(selected);
    setAnnouncement(`Destination ${selected.displayName} selected.`);
  };

  const selectPage = useCallback((pageIndex: number, event: React.MouseEvent) => {
    setSelectedOrder((current) => {
      let next = [...current];
      if (event.shiftKey && selectionAnchor !== null) {
        const from = Math.min(selectionAnchor, pageIndex);
        const to = Math.max(selectionAnchor, pageIndex);
        for (let index = from; index <= to && next.length < PDF_TO_IMAGES_MAX_OUTPUTS; index += 1) {
          if (!next.includes(index)) next.push(index);
        }
      } else if (event.ctrlKey || event.metaKey) {
        next = next.includes(pageIndex)
          ? next.filter((value) => value !== pageIndex)
          : next.length < PDF_TO_IMAGES_MAX_OUTPUTS ? [...next, pageIndex] : next;
      } else {
        next = [pageIndex];
      }
      setAnnouncement(`${next.length} page${next.length === 1 ? '' : 's'} selected in output order.`);
      return next;
    });
    setSelectionAnchor(pageIndex);
    setFocusedPage(pageIndex);
  }, [selectionAnchor]);

  const navigateThumbnail = (pageIndex: number, direction: -1 | 1 | 'first' | 'last') => {
    if (!pdf) return;
    const target = direction === 'first' ? 0
      : direction === 'last' ? pdf.numPages - 1
        : Math.max(0, Math.min(pdf.numPages - 1, pageIndex + direction));
    setFocusedPage(target);
    thumbnailVirtualizer.scrollToIndex(target, { align: 'auto' });
    window.setTimeout(() => {
      thumbnailScrollRef.current
        ?.querySelector<HTMLButtonElement>(`[data-page-index="${target}"]`)
        ?.focus();
    }, 0);
  };

  const moveSelected = (pageIndex: number, direction: -1 | 1) => {
    setSelectedOrder((current) => {
      const from = current.indexOf(pageIndex);
      const to = from + direction;
      if (from < 0 || to < 0 || to >= current.length) return current;
      const next = [...current];
      [next[from], next[to]] = [next[to], next[from]];
      setAnnouncement(`Source page ${pageIndex + 1} moved to output position ${to + 1}.`);
      return next;
    });
  };

  const selectFirst128 = () => {
    if (!pdf) return;
    const count = Math.min(pdf.numPages, PDF_TO_IMAGES_MAX_OUTPUTS);
    setSelectedOrder(Array.from({ length: count }, (_, index) => index));
    setSelectionAnchor(count - 1);
    setAnnouncement(`${count} pages selected. The v1 maximum is 128 outputs.`);
  };

  const validation = !pdf || !metadata ? 'Open one unencrypted local PDF.'
    : selectedOrder.length < 1 ? 'Select at least one page.'
      : selectedOrder.length > PDF_TO_IMAGES_MAX_OUTPUTS ? 'Select no more than 128 pages.'
        : !destination ? 'Choose a destination folder.'
          : !validOutputStem(stem) ? 'Enter a Windows-safe output stem up to 96 characters.'
            : null;

  const start = async () => {
    if (!pdf || !metadata || !destination || validation || operationRef.current) return;
    const operation = new PdfToImagesOperation(api.jobs);
    operationRef.current = operation;
    renderAbortRef.current = operation.controller;
    setBusy(true);
    setError(null);
    setJob(null);
    let ownershipReconciled = true;
    try {
      const pages = await planPdfImagePages(pdf, selectedOrder, dpi, operation.signal);
      if (operation.signal.aborted) throw new DOMException('Rendering cancelled', 'AbortError');
      const created = await api.jobs.createPdfToImages({
        viewerSessionId: metadata.sessionId,
        viewerGeneration: metadata.generation,
        destinationGrantId: destination.grantId,
        sourcePageCount: pdf.numPages,
        pages,
        format,
        dpi,
        outputStem: stem,
      });
      operation.registerCreatedJob(created.job.id);
      jobIdRef.current = created.job.id;
      setJob(created.job);
      if (operation.signal.aborted) {
        const cancelled = await operation.reconcileAfterAbort();
        setJob(cancelled);
        setAnnouncement('Cancellation completed. Owned staging was reconciled; published images were preserved.');
        return;
      }
      setAnnouncement(`Rendering ${created.pages.length} selected page${created.pages.length === 1 ? '' : 's'} sequentially.`);
      const completed = await renderPdfImageJob(
        pdf,
        created,
        dpi,
        operation.signal,
        setJob,
        (task) => { renderTaskRef.current = task; },
      );
      setJob(completed);
      setAnnouncement('Every selected page was encoded, verified, and published.');
      await closeDocument();
      openButtonRef.current?.focus();
    } catch (reason) {
      if (operation.signal.aborted && operation.hasCreatedJob) {
        try {
          const cancelled = await operation.reconcileAfterAbort();
          setJob(cancelled);
          setAnnouncement('Cancellation completed. Owned staging was reconciled; published images were preserved.');
        } catch (reconciliationError) {
          ownershipReconciled = false;
          setError(readableError(reconciliationError));
        }
      } else if (operation.hasCreatedJob) {
        const activeJobId = jobIdRef.current;
        if (activeJobId && operation.ownsJob(activeJobId)) {
          const snapshot = await api.jobs.get({ jobId: activeJobId }).catch(() => null);
          if (snapshot) setJob(snapshot);
        }
      }
      if (!operation.signal.aborted) {
        setError(readableError(reason));
      }
    } finally {
      operation.markFrontendSettled();
      if (ownershipReconciled) {
        try {
          await operation.waitForFrontendCleanup();
          releaseOperation(operation);
        } catch (cleanupError) {
          setError(readableError(cleanupError));
        }
      }
    }
  };

  const cancel = async () => {
    const operation = operationRef.current;
    if (!operation) return;
    renderTaskRef.current?.cancel();
    resourceRef.current?.transport.abort();
    const cancellation = operation.requestCancellation();
    const cleanup = operation.startFrontendCleanup(() => closeDocument(true));
    let jobReconciled = false;
    try {
      await cancellation;
      setAnnouncement(operation.hasCreatedJob
        ? 'Cancellation requested. Owned staging is being reconciled; published images are preserved.'
        : 'Cancellation requested before native job creation completed.');
      if (operation.hasCreatedJob) {
        const cancelled = await operation.reconcileAfterAbort();
        if (operationRef.current === operation) setJob(cancelled);
        jobReconciled = true;
      }
    } catch (reason) {
      setError(readableError(reason));
    }
    try {
      await cleanup;
      await operation.waitForFrontendCleanup();
    } catch (cleanupError) {
      setError(readableError(cleanupError));
      return;
    }
    if (jobReconciled && operation.isFrontendSettled) releaseOperation(operation);
    if (operation.isFrontendSettled) openButtonRef.current?.focus();
  };

  const preview = useMemo(() => selectedOrder.map((sourcePageIndex, ordinal) => ({
    sourcePageIndex,
    name: `${stem || 'document'}-page-${String(ordinal + 1).padStart(4, '0')}.${extension(format)}`,
  })), [format, selectedOrder, stem]);
  const progress = job?.progress;
  const progressPercent = progress && progress.totalUnits > 0
    ? Math.min(100, Math.round((progress.completedUnits / progress.totalUnits) * 100))
    : 0;
  const lastError = job?.errors.at(-1);

  return (
    <>
      {error && <div className="error-banner" role="alert">{error}</div>}
      <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
      <section className="pdf-images-layout" aria-label="PDF to images workspace">
        <article className="card pdf-page-picker">
          <div className="card-heading">
            <div><p className="eyebrow">OPAQUE PDF SESSION</p><h2>Select pages and output order</h2></div>
            <span className="status-chip">PDF.js 6.2.108</span>
          </div>
          <div className="selection-row compact-selection">
            <div><span className="field-label">Source PDF</span><strong>{metadata?.displayName ?? 'No PDF open'}</strong><small>{pdf ? `${pdf.numPages} pages · source path stays outside React` : 'One unencrypted local PDF'}</small></div>
            <button ref={openButtonRef} type="button" className="secondary" onClick={openPdf} disabled={loading || busy}>{loading ? 'Opening…' : metadata ? 'Replace' : 'Open PDF'}</button>
          </div>
          <div className="thumbnail-actions">
            <button type="button" className="secondary compact" disabled={!pdf || busy} onClick={selectFirst128}>Select first {Math.min(pdf?.numPages ?? 128, 128)}</button>
            <button type="button" className="secondary compact" disabled={selectedOrder.length === 0 || busy} onClick={() => setSelectedOrder([])}>Clear</button>
            <span>{selectedOrder.length} / 128 selected</span>
          </div>
          <div ref={thumbnailScrollRef} className="pdf-image-thumbnail-scroll" role={pdf ? 'list' : undefined} aria-label="PDF pages; use Ctrl or Shift to select multiple pages">
            {pdf ? <div style={{ height: thumbnailVirtualizer.getTotalSize(), position: 'relative' }}>
              {thumbnailVirtualizer.getVirtualItems().map((item) => <div role="listitem" key={item.key} ref={thumbnailVirtualizer.measureElement} data-index={item.index} style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}>
                <PageThumbnail
                  document={pdf}
                  pageIndex={item.index}
                  pageCount={pdf.numPages}
                  selected={selectedOrder.includes(item.index)}
                  current={focusedPage === item.index}
                  onSelect={selectPage}
                  onNavigate={navigateThumbnail}
                  onReorder={moveSelected}
                  onFocusPage={setFocusedPage}
                />
              </div>)}
            </div> : <p className="panel-empty">Open a PDF to reuse the verified thumbnail and keyboard-selection surface.</p>}
          </div>
          <div className="page-only-notice" role="note"><strong>Fixed rendering boundary</strong><p>Pages render sequentially at DPI ÷ 72 with print intent, disabled annotations/forms/XFA/scripting, and an opaque-white background. Raw RGBA uses authenticated binary IPC; Rust performs durable encoding.</p></div>
        </article>
        <aside className="side-stack">
          <article className="card output-card">
            <p className="eyebrow">BOUNDED IMAGE OUTPUT</p><h2>Format and density</h2>
            <fieldset className="choice-grid" disabled={busy}><legend>Format</legend>{(['jpeg', 'png', 'webp'] as const).map((value) => <label key={value}><input type="radio" name="pdf-image-format" checked={format === value} onChange={() => setFormat(value)} />{value === 'webp' ? 'WebP · lossless' : value.toUpperCase()}</label>)}</fieldset>
            <fieldset className="choice-grid" disabled={busy}><legend>DPI</legend>{([72, 150, 300] as const).map((value) => <label key={value}><input type="radio" name="pdf-image-dpi" checked={dpi === value} onChange={() => setDpi(value)} />{value}</label>)}</fieldset>
            <div className="selection-row compact-selection"><div><span className="field-label">Destination</span><strong>{destination?.displayName ?? 'No folder selected'}</strong><small>Opaque grant only · existing files are never overwritten</small></div><button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose</button></div>
            <label className="output-field"><span className="field-label">Output stem</span><input value={stem} onChange={(event) => setStem(event.target.value)} disabled={busy} /><small>Example: {preview[0]?.name ?? `${stem || 'document'}-page-0001.${extension(format)}`}</small></label>
            {validation && <p className="preflight-message" role="status">{validation}</p>}
            <div className="action-row"><button type="button" className="primary" disabled={Boolean(validation) || busy} onClick={start}>Convert pages</button>{busy && <button type="button" className="secondary danger" onClick={cancel}>Cancel</button>}</div>
          </article>
          <article className="card output-preview-card">
            <div className="card-heading"><h2>Output preview</h2><span>{preview.length} files</span></div>
            <ol className="output-preview-list" aria-label="Selected pages in output order">
              {preview.length === 0 && <li className="empty-inputs">No pages selected.</li>}
              {preview.map((item, ordinal) => <li key={item.sourcePageIndex}><span className="ordinal">{ordinal + 1}</span><div><strong>{item.name}</strong><small>Source page {item.sourcePageIndex + 1}</small></div><div className="row-actions"><button type="button" className="icon-button" disabled={busy || ordinal === 0} onClick={() => moveSelected(item.sourcePageIndex, -1)} aria-label={`Move source page ${item.sourcePageIndex + 1} up`}>↑</button><button type="button" className="icon-button" disabled={busy || ordinal === preview.length - 1} onClick={() => moveSelected(item.sourcePageIndex, 1)} aria-label={`Move source page ${item.sourcePageIndex + 1} down`}>↓</button></div></li>)}
            </ol>
          </article>
          <article className="card job-card" aria-live="polite" aria-atomic="true">
            <div className="card-heading"><h2>Per-output verification</h2>{job && <span className={`job-state state-${job.state}`}>{job.state}</span>}</div>
            <strong>{job ? `${job.progress.completedUnits} of ${job.progress.totalUnits} pages encoded and verified` : 'Ready for bounded local rendering'}</strong>
            <progress value={progressPercent} max={100} aria-label="PDF to images progress" />
            {job?.outputs.map((output) => <div className="output-status-row" key={output.ordinal}><span>{output.resolvedName ?? output.requestedName}</span><strong>{output.status}</strong></div>)}
            {(job?.state === 'failed' || job?.state === 'cancelled' || job?.state === 'interrupted') && <div className="failure-result" role="alert"><strong>{lastError?.title ?? `Conversion ${job.state}`}</strong><span>{lastError?.detail ?? 'No unverified output was published.'}</span></div>}
            {job?.errors.some((item) => item.code === 'PARTIAL_PUBLICATION') && <div className="failure-result" role="alert"><strong>Partial publication</strong><span>Published user images were preserved; unpublished owned staging was reconciled.</span></div>}
          </article>
        </aside>
      </section>
    </>
  );
}
