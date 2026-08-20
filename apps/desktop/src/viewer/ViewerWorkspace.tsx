import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type {
  CorePdfOperationId,
  DestinationGrant,
  JobRecord,
  OperationPlanEnvelope,
  OutputRotation,
  ProgressEvent,
  SplitOutputRange,
  ViewerDocumentMetadata,
} from '@document-studio/contracts';
import type { PDFDocumentProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { api, operationErrorMessage } from '../api';
import { PageSurface, PageThumbnail, type FitMode } from './PageSurface';
import { PdfTextIndexer, type SearchSnapshot } from './pdfSearch';
import { loadPdfSession, PasswordResponses, type LoadedPdfSession, type PdfPasswordChallenge } from './pdfSession';

type ViewerState = 'empty' | 'selecting' | 'loading' | 'ready' | 'password' | 'error' | 'source-changed';
type SplitMode = 'every-page' | 'fixed-count' | 'ranges';

const TERMINAL_STATES = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const OPERATION_LABELS: Record<CorePdfOperationId, string> = {
  'pdf.extract-pages': 'Extract pages',
  'pdf.remove-pages': 'Remove pages',
  'pdf.reorder-pages': 'Reorder pages',
  'pdf.rotate-pages': 'Rotate pages',
  'pdf.split': 'Split PDF',
};

const EMPTY_SEARCH: SearchSnapshot = {
  query: '', resultPages: new Map(), resultCount: 0, searchedPages: 0,
  totalPages: 0, stillSearching: false, limited: false, imageOnlyPages: 0,
};

function validOutputName(name: string): boolean {
  return name.length > 4 && name.length <= 255
    && name.toLocaleLowerCase().endsWith('.pdf')
    && !/[<>:"/\\|?*\u0000-\u001f]/u.test(name)
    && !/[. ]$/u.test(name)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(name);
}

function baseName(displayName: string): string {
  return displayName.replace(/\.pdf$/iu, '').replace(/[^\p{L}\p{N}._ -]+/gu, '-').slice(0, 96) || 'document';
}

function rangeName(base: string, ordinal: number): string {
  return `${base}-part-${String(ordinal + 1).padStart(3, '0')}.pdf`;
}

function explicitRanges(value: string, pageCount: number, base: string): SplitOutputRange[] | null {
  const tokens = value.split(',').map((token) => token.trim()).filter(Boolean);
  const ranges: SplitOutputRange[] = [];
  for (const [ordinal, token] of tokens.entries()) {
    const match = /^(\d+)(?:\s*-\s*(\d+))?$/u.exec(token);
    if (!match) return null;
    const start = Number(match[1]);
    const end = Number(match[2] ?? match[1]);
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 1 || end < start || end > pageCount) return null;
    ranges.push({ startPageIndex: start - 1, endPageIndex: end - 1, outputName: rangeName(base, ordinal) });
  }
  if (ranges.length < 1 || ranges.length > 128) return null;
  let expected = 0;
  for (const range of ranges) {
    if (range.startPageIndex !== expected) return null;
    expected = range.endPageIndex + 1;
  }
  return expected === pageCount ? ranges : null;
}

function splitRanges(
  mode: SplitMode,
  pageCount: number,
  fixedCount: number,
  rangeText: string,
  base: string,
): SplitOutputRange[] | null {
  if (mode === 'ranges') return explicitRanges(rangeText, pageCount, base);
  const chunk = mode === 'every-page' ? 1 : fixedCount;
  if (!Number.isSafeInteger(chunk) || chunk < 1) return null;
  const outputCount = Math.ceil(pageCount / chunk);
  if (outputCount > 128) return null;
  return Array.from({ length: outputCount }, (_, ordinal) => ({
    startPageIndex: ordinal * chunk,
    endPageIndex: Math.min(pageCount - 1, (ordinal + 1) * chunk - 1),
    outputName: rangeName(base, ordinal),
  }));
}

function pageForResult(snapshot: SearchSnapshot, resultIndex: number): number | null {
  if (snapshot.resultCount === 0) return null;
  let remaining = ((resultIndex % snapshot.resultCount) + snapshot.resultCount) % snapshot.resultCount;
  for (const [pageIndex, offsets] of [...snapshot.resultPages.entries()].sort((a, b) => a[0] - b[0])) {
    if (remaining < offsets.length) return pageIndex;
    remaining -= offsets.length;
  }
  return null;
}

export function ViewerWorkspace() {
  const [viewerState, setViewerState] = useState<ViewerState>('empty');
  const [metadata, setMetadata] = useState<ViewerDocumentMetadata | null>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const [firstPageReady, setFirstPageReady] = useState(false);
  const [passwordChallenge, setPasswordChallenge] = useState<PdfPasswordChallenge | null>(null);
  const [password, setPassword] = useState('');
  const [encryptedForViewing, setEncryptedForViewing] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [fitMode, setFitMode] = useState<FitMode>('fit-width');
  const [viewRotation, setViewRotation] = useState(0);
  const [containerSize, setContainerSize] = useState({ width: 900, height: 700 });
  const [selectedPages, setSelectedPages] = useState<Set<number>>(new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<number | null>(null);
  const [pageOrder, setPageOrder] = useState<number[]>([]);
  const [operationId, setOperationId] = useState<CorePdfOperationId>('pdf.extract-pages');
  const [outputName, setOutputName] = useState('extracted-pages.pdf');
  const [outputRotation, setOutputRotation] = useState<OutputRotation>(90);
  const [splitMode, setSplitMode] = useState<SplitMode>('fixed-count');
  const [fixedCount, setFixedCount] = useState(10);
  const [rangeText, setRangeText] = useState('');
  const [destination, setDestination] = useState<DestinationGrant | null>(null);
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchSnapshot, setSearchSnapshot] = useState<SearchSnapshot>(EMPTY_SEARCH);
  const [activeResult, setActiveResult] = useState(0);
  const [indexer, setIndexer] = useState<PdfTextIndexer | null>(null);
  const sessionRef = useRef<LoadedPdfSession | null>(null);
  const indexerRef = useRef<PdfTextIndexer | null>(null);
  const destinationRef = useRef<DestinationGrant | null>(null);
  const metadataRef = useRef<ViewerDocumentMetadata | null>(null);
  const loadAbortRef = useRef<AbortController | null>(null);
  const canvasScrollRef = useRef<HTMLDivElement>(null);
  const thumbnailScrollRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const loadSequence = useRef(0);

  const closeDocument = useCallback(async () => {
    loadSequence.current += 1;
    loadAbortRef.current?.abort();
    loadAbortRef.current = null;
    const loaded = sessionRef.current;
    sessionRef.current = null;
    if (loaded) await loaded.close().catch(() => undefined);
    const currentMetadata = metadataRef.current;
    metadataRef.current = null;
    if (currentMetadata) {
      await api.viewer.close({
        sessionId: currentMetadata.sessionId,
        generation: currentMetadata.generation,
      }).catch(() => undefined);
    }
    indexerRef.current?.destroy();
    indexerRef.current = null;
    if (destinationRef.current) {
      await api.viewer.revokeDestination(destinationRef.current.grantId).catch(() => undefined);
      destinationRef.current = null;
    }
    setIndexer(null);
    setPdf(null);
    setMetadata(null);
    setPassword('');
    setPasswordChallenge(null);
    setEncryptedForViewing(false);
    setSelectedPages(new Set());
    setPageOrder([]);
    setDestination(null);
    setJob(null);
    setProgress(null);
    setFirstPageReady(false);
    setSearchQuery('');
    setSearchSnapshot(EMPTY_SEARCH);
    setViewerState('empty');
    setError(null);
  }, []);

  const loadMetadata = useCallback(async (nextMetadata: ViewerDocumentMetadata) => {
    await closeDocument();
    const sequence = ++loadSequence.current;
    metadataRef.current = nextMetadata;
    setMetadata(nextMetadata);
    setViewerState('loading');
    setError(null);
    setFirstPageReady(false);
    performance.mark('g03-viewer-session-received');
    const abortController = new AbortController();
    loadAbortRef.current = abortController;
    try {
      const loaded = await loadPdfSession(
        nextMetadata,
        (challenge) => {
          if (sequence !== loadSequence.current) return;
          setEncryptedForViewing(true);
          setPassword('');
          setPasswordChallenge(challenge);
          setViewerState('password');
        },
        (reason) => {
          if (sequence !== loadSequence.current) return;
          setViewerState('source-changed');
          setError(operationErrorMessage(reason));
        },
        abortController.signal,
      );
      if (sequence !== loadSequence.current) {
        await loaded.close();
        return;
      }
      sessionRef.current = loaded;
      setPdf(loaded.document);
      setPageOrder(Array.from({ length: loaded.document.numPages }, (_, index) => index));
      setRangeText(`1-${loaded.document.numPages}`);
      setOutputName(`${baseName(nextMetadata.displayName)}-pages.pdf`);
      const nextIndexer = new PdfTextIndexer(loaded.document);
      nextIndexer.prioritize([0]);
      indexerRef.current = nextIndexer;
      setIndexer(nextIndexer);
      setSearchSnapshot(nextIndexer.getSnapshot());
      setPassword('');
      setPasswordChallenge(null);
      setViewerState('ready');
      performance.mark('g03-pdf-document-ready');
    } catch (reason) {
      if (sequence !== loadSequence.current) return;
      const message = operationErrorMessage(reason);
      setViewerState(message.toLocaleLowerCase().includes('changed') ? 'source-changed' : 'error');
      setError(message);
    }
  }, [closeDocument]);

  useEffect(() => {
    let active = true;
    let stopOpened: (() => void) | undefined;
    let stopFailed: (() => void) | undefined;
    void api.viewer.setDropEnabled(true);
    void api.viewer.onDocumentOpened((document) => {
      if (active) void loadMetadata(document);
    }).then((stop) => { if (active) stopOpened = stop; else stop(); });
    void api.viewer.onOpenFailed((reason) => {
      if (!active) return;
      setViewerState('error');
      setError(operationErrorMessage(reason));
    }).then((stop) => { if (active) stopFailed = stop; else stop(); });
    return () => {
      active = false;
      stopOpened?.();
      stopFailed?.();
      void api.viewer.setDropEnabled(false);
      void closeDocument();
    };
  }, [closeDocument, loadMetadata]);

  useEffect(() => {
    if (!job) return;
    let active = true;
    let stopProgress: (() => void) | undefined;
    void api.jobs.onProgress((event) => {
      if (!active || event.jobId !== job.id) return;
      setProgress(event);
      if (TERMINAL_STATES.has(event.state)) {
        void api.jobs.get({ jobId: event.jobId }).then((record) => active && setJob(record));
      }
    }).then((stop) => { if (active) stopProgress = stop; else stop(); });
    return () => { active = false; stopProgress?.(); };
  }, [job?.id]);

  useEffect(() => {
    if (!indexer) return;
    const update = () => setSearchSnapshot(indexer.getSnapshot());
    update();
    return indexer.subscribe(update);
  }, [indexer]);

  useEffect(() => {
    const element = canvasScrollRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setContainerSize({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [pdf]);

  const pageVirtualizer = useVirtualizer({
    useFlushSync: false,
    count: pageOrder.length,
    getScrollElement: () => canvasScrollRef.current,
    estimateSize: () => Math.max(420, 840 * (fitMode === 'custom' ? zoom : 1)),
    overscan: 2,
    useAnimationFrameWithResizeObserver: true,
    getItemKey: (index) => pageOrder[index] ?? index,
  });
  const thumbnailVirtualizer = useVirtualizer({
    useFlushSync: false,
    count: firstPageReady ? pageOrder.length : 0,
    getScrollElement: () => thumbnailScrollRef.current,
    estimateSize: () => 164,
    overscan: 6,
    useAnimationFrameWithResizeObserver: true,
    getItemKey: (index) => pageOrder[index] ?? index,
  });
  const virtualPages = pageVirtualizer.getVirtualItems();
  const currentVisualIndex = virtualPages[0]?.index ?? 0;
  const currentPage = pageOrder[currentVisualIndex] ?? 0;

  useEffect(() => {
    const visible = virtualPages.map((item) => pageOrder[item.index]).filter((value): value is number => value !== undefined);
    indexer?.prioritize(visible);
  }, [indexer, pageOrder, virtualPages]);

  const selectPage = useCallback((pageIndex: number, event: React.MouseEvent) => {
    setSelectedPages((current) => {
      const next = new Set(current);
      if (event.shiftKey && selectionAnchor !== null) {
        const from = pageOrder.indexOf(selectionAnchor);
        const to = pageOrder.indexOf(pageIndex);
        if (from >= 0 && to >= 0) {
          for (let index = Math.min(from, to); index <= Math.max(from, to); index += 1) next.add(pageOrder[index]);
        }
      } else if (event.ctrlKey || event.metaKey) {
        if (next.has(pageIndex)) next.delete(pageIndex); else next.add(pageIndex);
      } else {
        next.clear();
        next.add(pageIndex);
      }
      return next;
    });
    setSelectionAnchor(pageIndex);
    setAnnouncement(`Page ${pageIndex + 1} selected.`);
  }, [pageOrder, selectionAnchor]);

  const scrollToSourcePage = useCallback((pageIndex: number) => {
    const visualIndex = pageOrder.indexOf(pageIndex);
    if (visualIndex >= 0) {
      pageVirtualizer.scrollToIndex(visualIndex, { align: 'start' });
      window.setTimeout(() => pageVirtualizer.scrollToIndex(visualIndex, { align: 'start' }), 75);
      window.setTimeout(() => pageVirtualizer.scrollToIndex(visualIndex, { align: 'start' }), 225);
    }
  }, [pageOrder, pageVirtualizer]);

  const navigate = (visualIndex: number) => {
    const target = Math.max(0, Math.min(pageOrder.length - 1, visualIndex));
    const scroll = () => {
      if (target === 0 && canvasScrollRef.current) canvasScrollRef.current.scrollTop = 0;
      else if (target === pageOrder.length - 1 && canvasScrollRef.current) {
        canvasScrollRef.current.scrollTop = canvasScrollRef.current.scrollHeight;
      } else pageVirtualizer.scrollToIndex(target, { align: 'start' });
    };
    scroll();
    window.setTimeout(scroll, 75);
    window.setTimeout(scroll, 225);
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.ctrlKey && event.key.toLocaleLowerCase() === 'o') {
        event.preventDefault();
        void openDocument();
      } else if (event.ctrlKey && event.key.toLocaleLowerCase() === 'f') {
        event.preventDefault();
        setSearchOpen(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
      } else if (event.ctrlKey && (event.key === '+' || event.key === '=')) {
        event.preventDefault();
        setFitMode('custom'); setZoom((value) => Math.min(4, value + 0.1));
      } else if (event.ctrlKey && event.key === '-') {
        event.preventDefault();
        setFitMode('custom'); setZoom((value) => Math.max(0.25, value - 0.1));
      } else if (event.key === 'PageDown') {
        event.preventDefault(); navigate(currentVisualIndex + 1);
      } else if (event.key === 'PageUp') {
        event.preventDefault(); navigate(currentVisualIndex - 1);
      } else if (event.key === 'Home' && !(event.target instanceof HTMLInputElement)) {
        event.preventDefault(); navigate(0);
      } else if (event.key === 'End' && !(event.target instanceof HTMLInputElement)) {
        event.preventDefault(); navigate(pageOrder.length - 1);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  const openDocument = async () => {
    setViewerState('selecting');
    setError(null);
    performance.mark('g03-open-acknowledged');
    try {
      const selected = await api.viewer.open();
      if (selected) await loadMetadata(selected);
      else setViewerState(metadataRef.current ? 'ready' : 'empty');
    } catch (reason) {
      setViewerState('error');
      setError(operationErrorMessage(reason));
    }
  };

  const changeZoom = (value: number) => {
    const anchor = currentPage;
    setFitMode('custom');
    setZoom(Math.max(0.25, Math.min(4, value)));
    requestAnimationFrame(() => scrollToSourcePage(anchor));
  };

  const moveSelected = (direction: -1 | 1) => {
    if (selectedPages.size !== 1) return;
    const selected = [...selectedPages][0];
    const from = pageOrder.indexOf(selected);
    const to = from + direction;
    if (from < 0 || to < 0 || to >= pageOrder.length) return;
    setPageOrder((current) => {
      const next = [...current];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
    requestAnimationFrame(() => {
      pageVirtualizer.scrollToIndex(to, { align: 'center' });
      thumbnailScrollRef.current?.querySelector<HTMLButtonElement>(`[data-page-index="${selected}"]`)?.focus();
    });
    setAnnouncement(`Page ${selected + 1} moved to position ${to + 1}.`);
  };

  const navigateThumbnail = (pageIndex: number, direction: -1 | 1 | 'first' | 'last') => {
    const currentIndex = pageOrder.indexOf(pageIndex);
    const target = direction === 'first'
      ? 0
      : direction === 'last'
        ? pageOrder.length - 1
        : Math.max(0, Math.min(pageOrder.length - 1, currentIndex + direction));
    const targetPage = pageOrder[target];
    thumbnailVirtualizer.scrollToIndex(target, { align: 'center' });
    navigate(target);
    window.setTimeout(() => {
      thumbnailScrollRef.current
        ?.querySelector<HTMLButtonElement>(`[data-page-index="${targetPage}"]`)
        ?.focus();
    }, 100);
  };

  const updateSearch = (value: string) => {
    setSearchQuery(value);
    setActiveResult(0);
    indexer?.setQuery(value);
  };

  const moveSearchResult = (direction: -1 | 1) => {
    if (searchSnapshot.resultCount === 0) return;
    const next = (activeResult + direction + searchSnapshot.resultCount) % searchSnapshot.resultCount;
    setActiveResult(next);
    const page = pageForResult(searchSnapshot, next);
    if (page !== null) scrollToSourcePage(page);
  };

  const chooseDestination = async () => {
    const grant = await api.viewer.chooseDestination();
    if (!grant) return;
    if (destination) await api.viewer.revokeDestination(destination.grantId).catch(() => undefined);
    destinationRef.current = grant;
    setDestination(grant);
  };

  const plan = useMemo<OperationPlanEnvelope | null>(() => {
    if (!pdf || encryptedForViewing || pdf.numPages > 4096) return null;
    const selectedInOrder = pageOrder.filter((pageIndex) => selectedPages.has(pageIndex));
    const envelope = { schemaVersion: 1 as const, operationId, sourcePageCount: pdf.numPages };
    if (operationId === 'pdf.extract-pages') {
      return selectedInOrder.length > 0 && validOutputName(outputName)
        ? { ...envelope, operationId, payload: { selectedPageIndexes: selectedInOrder, outputName } }
        : null;
    }
    if (operationId === 'pdf.remove-pages') {
      return selectedPages.size > 0 && selectedPages.size < pdf.numPages && validOutputName(outputName)
        ? { ...envelope, operationId, payload: { removedPageIndexes: [...selectedPages].sort((a, b) => a - b), outputName } }
        : null;
    }
    if (operationId === 'pdf.reorder-pages') {
      return validOutputName(outputName)
        ? { ...envelope, operationId, payload: { orderedPageIndexes: pageOrder, outputName } }
        : null;
    }
    if (operationId === 'pdf.rotate-pages') {
      return selectedInOrder.length > 0 && validOutputName(outputName)
        ? { ...envelope, operationId, payload: {
          rotations: selectedInOrder.map((pageIndex) => ({ pageIndex, clockwiseDegrees: outputRotation })), outputName,
        } }
        : null;
    }
    const ranges = splitRanges(splitMode, pdf.numPages, fixedCount, rangeText, baseName(metadata?.displayName ?? 'document'));
    return ranges ? { ...envelope, operationId, payload: { ranges } } : null;
  }, [encryptedForViewing, fixedCount, metadata?.displayName, operationId, outputName, outputRotation, pageOrder, pdf, rangeText, selectedPages, splitMode]);

  const runOperation = async () => {
    if (!plan || !destination || !metadata) return;
    setError(null);
    setProgress(null);
    try {
      const record = await api.jobs.createCorePdf({
        viewerSessionId: metadata.sessionId,
        viewerGeneration: metadata.generation,
        destinationGrantId: destination.grantId,
        plan,
      });
      setJob(record);
      setAnnouncement(`${OPERATION_LABELS[operationId]} started as a durable local job.`);
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const busy = Boolean(job && !TERMINAL_STATES.has(job.state));
  const selectionCount = selectedPages.size;
  const planHelp = encryptedForViewing
    ? 'Encrypted PDFs may be viewed with an in-memory password, but G03 output operations are unavailable.'
    : pdf && pdf.numPages > 4096
    ? 'Viewing is available, but G03 output plans are limited to 4,096 pages.'
    : !destination ? 'Choose a destination folder.'
      : !plan ? operationId === 'pdf.split'
        ? 'Split ranges must cover every page once, in order, with at most 128 outputs.'
        : 'Select the required pages and enter a safe PDF filename.'
        : null;

  return (
    <main className="viewer-workspace" aria-label="PDF viewer and page organizer">
      <div className="sr-announcement" aria-live="polite" aria-atomic="true">{announcement}</div>
      <header className="viewer-toolbar" aria-label="Viewer toolbar">
        <button type="button" className="primary compact" onClick={() => void openDocument()} disabled={viewerState === 'loading' || viewerState === 'selecting'}>Open PDF</button>
        {metadata && <strong className="viewer-document-name" title={metadata.displayName}>{metadata.displayName}</strong>}
        <div className="toolbar-group" aria-label="Page navigation">
          <button type="button" className="icon-button" aria-label="First page" onClick={() => navigate(0)} disabled={!pdf}>⇤</button>
          <button type="button" className="icon-button" aria-label="Previous page" onClick={() => navigate(currentVisualIndex - 1)} disabled={!pdf}>←</button>
          <label className="page-number-field"><span className="sr-only">Page number</span><input type="number" min={1} max={pageOrder.length || 1} value={pdf ? currentVisualIndex + 1 : 1} onChange={(event) => navigate(Number(event.target.value) - 1)} disabled={!pdf} /><span>of {pageOrder.length || 0}</span></label>
          <button type="button" className="icon-button" aria-label="Next page" onClick={() => navigate(currentVisualIndex + 1)} disabled={!pdf}>→</button>
          <button type="button" className="icon-button" aria-label="Last page" onClick={() => navigate(pageOrder.length - 1)} disabled={!pdf}>⇥</button>
        </div>
        <div className="toolbar-group" aria-label="Zoom and fit">
          <button type="button" className="icon-button" aria-label="Zoom out" onClick={() => changeZoom(zoom - 0.1)} disabled={!pdf}>−</button>
          <span className="zoom-value">{Math.round(zoom * 100)}%</span>
          <button type="button" className="icon-button" aria-label="Zoom in" onClick={() => changeZoom(zoom + 0.1)} disabled={!pdf}>+</button>
          <button type="button" className="secondary compact" onClick={() => setFitMode('actual')} disabled={!pdf}>Actual size</button>
          <button type="button" className="secondary compact" onClick={() => setFitMode('fit-page')} disabled={!pdf}>Fit page</button>
          <button type="button" className="secondary compact" onClick={() => setFitMode('fit-width')} disabled={!pdf}>Fit width</button>
          <button type="button" className="icon-button" aria-label="Rotate view clockwise" onClick={() => setViewRotation((value) => (value + 90) % 360)} disabled={!pdf}>↻</button>
        </div>
        <button type="button" className="secondary compact" aria-expanded={searchOpen} onClick={() => { setSearchOpen(true); requestAnimationFrame(() => searchInputRef.current?.focus()); }} disabled={!pdf}>Search</button>
        {pdf && <button type="button" className="secondary compact" onClick={() => void closeDocument()}>Close</button>}
      </header>

      {searchOpen && pdf && <section className="search-bar" aria-label="Search document">
        <label><span className="sr-only">Search text</span><input ref={searchInputRef} value={searchQuery} onChange={(event) => updateSearch(event.target.value)} placeholder="Find in document" /></label>
        <button type="button" className="icon-button" aria-label="Previous search result" onClick={() => moveSearchResult(-1)} disabled={searchSnapshot.resultCount === 0}>↑</button>
        <button type="button" className="icon-button" aria-label="Next search result" onClick={() => moveSearchResult(1)} disabled={searchSnapshot.resultCount === 0}>↓</button>
        <span role="status">{searchSnapshot.resultCount === 0 ? 'No results yet' : `${activeResult + 1} of ${searchSnapshot.resultCount}`}{searchSnapshot.stillSearching ? ' · still searching' : ''}{searchSnapshot.limited ? ' · result limit reached' : ''}</span>
        {searchQuery && !searchSnapshot.stillSearching && searchSnapshot.resultCount === 0 && searchSnapshot.imageOnlyPages > 0 && <span>Searchable text is unavailable on image-only pages. OCR is a later goal.</span>}
        <button type="button" className="icon-button" aria-label="Close search" onClick={() => { setSearchOpen(false); updateSearch(''); }}>×</button>
      </section>}

      {error && <div className="error-banner viewer-error" role="alert">{error}</div>}
      <div className="viewer-body">
        <aside className="thumbnail-panel" aria-label="Pages and selection">
          <div className="panel-heading"><strong>Pages</strong><span>{selectionCount} selected</span></div>
          <div ref={thumbnailScrollRef} className="thumbnail-scroll" role="list" aria-label="PDF pages; use Ctrl or Shift to select multiple pages">
            {pdf && firstPageReady ? <div style={{ height: thumbnailVirtualizer.getTotalSize(), position: 'relative' }}>
              {thumbnailVirtualizer.getVirtualItems().map((item) => {
                const pageIndex = pageOrder[item.index];
                return <div role="listitem" key={item.key} ref={thumbnailVirtualizer.measureElement} data-index={item.index} style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}>
                  <PageThumbnail document={pdf} pageIndex={pageIndex} pageCount={pdf.numPages} selected={selectedPages.has(pageIndex)} current={currentPage === pageIndex} onSelect={selectPage} onNavigate={navigateThumbnail} />
                </div>;
              })}
            </div> : <p className="panel-empty">{viewerState === 'loading' ? 'First page is loading…' : 'Open one local PDF.'}</p>}
          </div>
          <div className="thumbnail-actions" aria-label="Accessible page reorder controls">
            <button type="button" className="secondary compact" onClick={() => moveSelected(-1)} disabled={selectionCount !== 1 || busy}>Move up</button>
            <button type="button" className="secondary compact" onClick={() => moveSelected(1)} disabled={selectionCount !== 1 || busy}>Move down</button>
          </div>
        </aside>

        <section ref={canvasScrollRef} className="page-canvas-scroll" aria-label="Document pages" tabIndex={0}>
          {!pdf && <div className="viewer-empty-state">
            <p className="eyebrow">LOCAL PDF WORKSPACE</p><h1>Open, inspect and organize one PDF</h1>
            <p>Choose a file or drop one PDF here. The source path stays in Rust; only an opaque session reaches this workspace.</p>
            <button type="button" className="primary" onClick={() => void openDocument()}>Open a PDF</button>
            {viewerState === 'loading' && <p role="status">Loading the first visible page…</p>}
          </div>}
          {pdf && <div className="virtual-page-stack" style={{ height: pageVirtualizer.getTotalSize(), position: 'relative' }}>
            {virtualPages.map((item) => {
              const pageIndex = pageOrder[item.index];
              return <div key={item.key} ref={pageVirtualizer.measureElement} data-index={item.index} className="virtual-page-row" style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}>
                <PageSurface document={pdf} pageIndex={pageIndex} pageCount={pdf.numPages} zoom={zoom} fitMode={fitMode} availableWidth={containerSize.width} availableHeight={containerSize.height} viewRotation={viewRotation} selected={selectedPages.has(pageIndex)} searchQuery={searchQuery} searchHits={searchSnapshot.resultPages.get(pageIndex)?.length ?? 0} onSelect={selectPage} onRendered={(renderedIndex) => {
                  if (renderedIndex === pageOrder[0] && !firstPageReady) {
                    setFirstPageReady(true); performance.mark('g03-first-page-displayed');
                  }
                }} />
              </div>;
            })}
          </div>}
        </section>

        <aside className="organizer-inspector" aria-label="Page operation settings">
          <p className="eyebrow">PAGE ORGANIZER</p><h2>Prepare output</h2>
          <label className="inspector-field"><span>Operation</span><select value={operationId} onChange={(event) => setOperationId(event.target.value as CorePdfOperationId)} disabled={!pdf || busy}>{Object.entries(OPERATION_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
          {operationId !== 'pdf.split' && <label className="inspector-field"><span>Output filename</span><input value={outputName} onChange={(event) => setOutputName(event.target.value)} disabled={!pdf || busy} /></label>}
          {operationId === 'pdf.rotate-pages' && <label className="inspector-field"><span>Clockwise output rotation</span><select value={outputRotation} onChange={(event) => setOutputRotation(Number(event.target.value) as OutputRotation)} disabled={!pdf || busy}><option value={90}>90°</option><option value={180}>180°</option><option value={270}>270°</option></select></label>}
          {operationId === 'pdf.split' && <>
            <label className="inspector-field"><span>Split mode</span><select value={splitMode} onChange={(event) => setSplitMode(event.target.value as SplitMode)} disabled={!pdf || busy}><option value="every-page">Every page</option><option value="fixed-count">Fixed page count</option><option value="ranges">Explicit ranges</option></select></label>
            {splitMode === 'fixed-count' && <label className="inspector-field"><span>Pages per output</span><input type="number" min={1} value={fixedCount} onChange={(event) => setFixedCount(Number(event.target.value))} /></label>}
            {splitMode === 'ranges' && <label className="inspector-field"><span>Ranges</span><input value={rangeText} onChange={(event) => setRangeText(event.target.value)} placeholder="1-3, 4-8" /><small>Ranges are 1-based and must cover every page exactly once.</small></label>}
          </>}
          <div className="selection-summary"><strong>{selectionCount} selected</strong><span>UI pages are 1-based; stored page indexes are 0-based.</span></div>
          <button type="button" className="secondary" onClick={() => void chooseDestination()} disabled={!pdf || busy}>{destination ? `Destination: ${destination.displayName}` : 'Choose destination'}</button>
          {planHelp && <p className="preflight-message" role="status">{planHelp}</p>}
          <button type="button" className="primary" onClick={() => void runOperation()} disabled={!plan || !destination || busy}>Apply / Export</button>
          <p className="inspector-note">Navigation, search and selection stay ephemeral. A durable job is created only when Apply / Export is pressed.</p>
        </aside>
      </div>

      {passwordChallenge && <div className="modal-backdrop" role="presentation"><form className="password-dialog" role="dialog" aria-modal="true" aria-labelledby="password-title" onSubmit={(event) => {
        event.preventDefault();
        const submitted = password;
        setPassword('');
        setPasswordChallenge(null);
        setViewerState('loading');
        passwordChallenge.submit(submitted);
      }}><h2 id="password-title">Password required</h2><p>{passwordChallenge.reason === PasswordResponses.INCORRECT_PASSWORD ? 'That password was not accepted. Try again.' : 'Enter the PDF password to view this document.'}</p><label className="inspector-field"><span>Password</span><input type="password" autoFocus value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="off" /></label><div className="action-row"><button type="submit" className="primary">Unlock in memory</button><button type="button" className="secondary" onClick={() => void closeDocument()}>Cancel</button></div><small>The password is not logged, persisted or passed to qpdf. Structural operations remain unavailable for encrypted PDFs.</small></form></div>}

      <footer className="viewer-job-tray" aria-label="Durable job tray" aria-live="polite">
        <div><strong>{job ? OPERATION_LABELS[job.operationId as CorePdfOperationId] ?? job.operationId : 'No durable page job'}</strong><span>{progress?.message ?? (job ? `Job ${job.state}` : 'Viewer activity is not job history.')}</span></div>
        {job && <span className={`job-state state-${job.state}`}>{job.state}</span>}
        {job && !TERMINAL_STATES.has(job.state) && <button type="button" className="secondary compact" disabled={progress?.cancellable === false || job.state === 'publishing'} onClick={() => void api.jobs.cancel({ jobId: job.id })}>Cancel</button>}
        {job?.state === 'completed' && <span>{job.outputs.length} verified output{job.outputs.length === 1 ? '' : 's'} published</span>}
        {job?.errors.at(-1) && <span className="job-tray-error">{job.errors.at(-1)?.title}: {job.errors.at(-1)?.detail}</span>}
      </footer>
    </main>
  );
}
