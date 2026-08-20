import { memo, useEffect, useRef, useState } from 'react';
import {
  TextLayer,
  type PDFDocumentProxy,
  type RenderTask,
} from 'pdfjs-dist/legacy/build/pdf.mjs';
import {
  MAX_CANVAS_PIXELS,
  PAGE_DPR_CAP,
  SAFE_PAGE_RENDER_OPTIONS,
  THUMBNAIL_DPR_CAP,
} from './pdfSession';
import { normalizeSearchText } from './pdfSearch';

export type FitMode = 'custom' | 'actual' | 'fit-page' | 'fit-width';
const MAX_TEXT_LAYER_ITEMS = 100_000;
const MAX_TEXT_LAYER_CHARACTERS = 1_000_000;

interface PageSurfaceProps {
  document: PDFDocumentProxy;
  pageIndex: number;
  pageCount: number;
  zoom: number;
  fitMode: FitMode;
  availableWidth: number;
  availableHeight: number;
  viewRotation: number;
  selected: boolean;
  searchQuery: string;
  searchHits: number;
  onSelect(pageIndex: number, event: React.MouseEvent): void;
  onRendered(pageIndex: number): void;
}

function effectiveScale(
  mode: FitMode,
  zoom: number,
  pageWidth: number,
  pageHeight: number,
  availableWidth: number,
  availableHeight: number,
): number {
  if (mode === 'actual') return 1;
  if (mode === 'fit-width') return Math.max(0.1, (availableWidth - 48) / pageWidth);
  if (mode === 'fit-page') {
    return Math.max(0.1, Math.min((availableWidth - 48) / pageWidth, (availableHeight - 72) / pageHeight));
  }
  return zoom;
}

function safeOutputScale(width: number, height: number, cap: number): number {
  const requested = Math.min(window.devicePixelRatio || 1, cap);
  const pixels = width * height * requested * requested;
  return pixels <= MAX_CANVAS_PIXELS
    ? requested
    : Math.max(0.25, Math.sqrt(MAX_CANVAS_PIXELS / Math.max(1, width * height)));
}

export const PageSurface = memo(function PageSurface({
  document,
  pageIndex,
  pageCount,
  zoom,
  fitMode,
  availableWidth,
  availableHeight,
  viewRotation,
  selected,
  searchQuery,
  searchHits,
  onSelect,
  onRendered,
}: PageSurfaceProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);
  const onRenderedRef = useRef(onRendered);
  const [dimensions, setDimensions] = useState({ width: 612, height: 792 });
  const [renderError, setRenderError] = useState(false);
  useEffect(() => { onRenderedRef.current = onRendered; }, [onRendered]);

  useEffect(() => {
    let active = true;
    let renderTask: RenderTask | null = null;
    let textLayer: TextLayer | null = null;
    const canvas = canvasRef.current;
    const textContainer = textLayerRef.current;
    if (!canvas || !textContainer) return;
    setRenderError(false);
    void document.getPage(pageIndex + 1).then(async (page) => {
      if (!active) return;
      const base = page.getViewport({ scale: 1, rotation: viewRotation });
      const scale = effectiveScale(
        fitMode,
        zoom,
        base.width,
        base.height,
        availableWidth,
        availableHeight,
      );
      const viewport = page.getViewport({ scale, rotation: viewRotation });
      const outputScale = safeOutputScale(viewport.width, viewport.height, PAGE_DPR_CAP);
      canvas.width = Math.max(1, Math.floor(viewport.width * outputScale));
      canvas.height = Math.max(1, Math.floor(viewport.height * outputScale));
      canvas.style.width = `${Math.floor(viewport.width)}px`;
      canvas.style.height = `${Math.floor(viewport.height)}px`;
      textContainer.style.width = `${Math.floor(viewport.width)}px`;
      textContainer.style.height = `${Math.floor(viewport.height)}px`;
      setDimensions({ width: viewport.width, height: viewport.height });
      const context = canvas.getContext('2d', { alpha: false });
      if (!context) throw new Error('Canvas unavailable');
      renderTask = page.render({
        canvas,
        canvasContext: context,
        viewport,
        transform: outputScale === 1 ? undefined : [outputScale, 0, 0, outputScale, 0, 0],
        annotationMode: SAFE_PAGE_RENDER_OPTIONS.annotationMode,
      });
      const textPromise = page.getTextContent({ disableNormalization: false }).then(async (content) => {
        if (!active || content.items.length > MAX_TEXT_LAYER_ITEMS) return;
        const characterCount = content.items.reduce(
          (total, item) => total + ('str' in item ? item.str.length : 0),
          0,
        );
        if (characterCount > MAX_TEXT_LAYER_CHARACTERS) return;
        textContainer.replaceChildren();
        textLayer = new TextLayer({ textContentSource: content, container: textContainer, viewport });
        await textLayer.render();
        if (!active || !searchQuery) return;
        const normalizedQuery = normalizeSearchText(searchQuery);
        if (!normalizedQuery) return;
        const spans = textLayer.textDivs.map((textDiv) => ({
          element: textDiv,
          text: `${normalizeSearchText(textDiv.textContent ?? '')} `,
        }));
        const searchable = spans.map((span) => span.text).join('');
        for (let searchFrom = 0; searchFrom <= searchable.length - normalizedQuery.length;) {
          const found = searchable.indexOf(normalizedQuery, searchFrom);
          if (found < 0) break;
          const foundEnd = found + normalizedQuery.length;
          let spanStart = 0;
          for (const span of spans) {
            const spanEnd = spanStart + span.text.length;
            if (spanEnd > found && spanStart < foundEnd) {
              span.element.classList.add('search-hit-text');
            }
            spanStart = spanEnd;
          }
          searchFrom = found + Math.max(1, normalizedQuery.length);
        }
      });
      await Promise.all([renderTask.promise, textPromise]);
      if (active) onRenderedRef.current(pageIndex);
    }).catch((error: unknown) => {
      if (active && !(error instanceof Error && error.name === 'RenderingCancelledException')) {
        setRenderError(true);
      }
    });
    return () => {
      active = false;
      renderTask?.cancel();
      textLayer?.cancel();
      textContainer.replaceChildren();
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [availableHeight, availableWidth, document, fitMode, pageIndex, searchQuery, viewRotation, zoom]);

  return (
    <article
      className={`pdf-page-surface${selected ? ' selected' : ''}${searchHits > 0 ? ' search-result-page' : ''}`}
      style={{ width: dimensions.width, minHeight: dimensions.height }}
      data-page-index={pageIndex}
      aria-label={`Page ${pageIndex + 1} of ${pageCount}${selected ? ', selected' : ''}`}
      aria-selected={selected}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest('.textLayer')) return;
        onSelect(pageIndex, event);
      }}
    >
      <canvas ref={canvasRef} aria-label={`Rendered page ${pageIndex + 1}`} />
      <div ref={textLayerRef} className="textLayer" aria-label={`Selectable text for page ${pageIndex + 1}`} />
      {searchHits > 0 && <span className="page-search-count" aria-label={`${searchHits} search results on page ${pageIndex + 1}`}>{searchHits}</span>}
      {renderError && <div className="page-render-error" role="status">Page {pageIndex + 1} could not be rendered.</div>}
    </article>
  );
});

interface ThumbnailProps {
  document: PDFDocumentProxy;
  pageIndex: number;
  pageCount: number;
  selected: boolean;
  current: boolean;
  onSelect(pageIndex: number, event: React.MouseEvent): void;
  onNavigate(pageIndex: number, direction: -1 | 1 | 'first' | 'last'): void;
}

export const PageThumbnail = memo(function PageThumbnail({
  document,
  pageIndex,
  pageCount,
  selected,
  current,
  onSelect,
  onNavigate,
}: ThumbnailProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let active = true;
    let renderTask: RenderTask | null = null;
    const canvas = canvasRef.current;
    if (!canvas) return;
    void document.getPage(pageIndex + 1).then(async (page) => {
      if (!active) return;
      const base = page.getViewport({ scale: 1 });
      const viewport = page.getViewport({ scale: 112 / base.width });
      const outputScale = safeOutputScale(viewport.width, viewport.height, THUMBNAIL_DPR_CAP);
      canvas.width = Math.max(1, Math.floor(viewport.width * outputScale));
      canvas.height = Math.max(1, Math.floor(viewport.height * outputScale));
      canvas.style.width = `${Math.floor(viewport.width)}px`;
      canvas.style.height = `${Math.floor(viewport.height)}px`;
      const context = canvas.getContext('2d', { alpha: false });
      if (!context) throw new Error('Canvas unavailable');
      renderTask = page.render({
        canvas,
        canvasContext: context,
        viewport,
        transform: outputScale === 1 ? undefined : [outputScale, 0, 0, outputScale, 0, 0],
        annotationMode: SAFE_PAGE_RENDER_OPTIONS.annotationMode,
      });
      await renderTask.promise;
      if (active && pageIndex === 0) performance.mark('g03-first-thumbnail-displayed');
    }).catch(() => undefined);
    return () => {
      active = false;
      renderTask?.cancel();
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [document, pageIndex]);
  return (
    <button
      type="button"
      className={`page-thumbnail${selected ? ' selected' : ''}${current ? ' current' : ''}`}
      aria-label={`Page ${pageIndex + 1} of ${pageCount}${selected ? ', selected' : ''}`}
      aria-pressed={selected}
      data-page-index={pageIndex}
      tabIndex={current ? 0 : -1}
      onClick={(event) => onSelect(pageIndex, event)}
      onKeyDown={(event) => {
        const direction = event.key === 'ArrowUp' || event.key === 'ArrowLeft'
          ? -1
          : event.key === 'ArrowDown' || event.key === 'ArrowRight'
            ? 1
            : event.key === 'Home'
              ? 'first'
              : event.key === 'End'
                ? 'last'
                : null;
        if (direction !== null) {
          event.preventDefault();
          onNavigate(pageIndex, direction);
        }
      }}
    >
      <canvas ref={canvasRef} aria-hidden="true" />
      <span>{pageIndex + 1}</span>
    </button>
  );
});
