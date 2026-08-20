import { memo, useEffect, useRef, useState } from 'react';
import {
  TextLayer,
  type PDFDocumentProxy,
  type RenderTask,
} from 'pdfjs-dist/legacy/build/pdf.mjs';
import {
  MAX_CANVAS_PIXELS,
  MAX_CANVAS_HEIGHT,
  MAX_CANVAS_WIDTH,
  MAX_PAGE_CSS_DIMENSION,
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
  visible: boolean;
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

export interface SafeCanvasAllocation {
  width: number;
  height: number;
  outputScale: number;
}

export function safeCanvasAllocation(
  viewportWidth: number,
  viewportHeight: number,
  devicePixelRatio: number,
  cap: number,
): SafeCanvasAllocation | null {
  if (!Number.isFinite(viewportWidth) || !Number.isFinite(viewportHeight)
    || viewportWidth < 1 || viewportHeight < 1
    || viewportWidth > MAX_PAGE_CSS_DIMENSION || viewportHeight > MAX_PAGE_CSS_DIMENSION) return null;
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const outputScale = Math.min(ratio, cap);
  const width = Math.ceil(viewportWidth * outputScale);
  const height = Math.ceil(viewportHeight * outputScale);
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height)
    || width < 1 || height < 1 || width > MAX_CANVAS_WIDTH || height > MAX_CANVAS_HEIGHT) return null;
  const pixels = width * height;
  if (!Number.isSafeInteger(pixels) || pixels > MAX_CANVAS_PIXELS) return null;
  return { width, height, outputScale };
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
  visible,
  onSelect,
  onRendered,
}: PageSurfaceProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);
  const onRenderedRef = useRef(onRendered);
  const [dimensions, setDimensions] = useState({ width: 612, height: 792 });
  const [renderError, setRenderError] = useState<'too-large' | 'failed' | null>(null);
  useEffect(() => { onRenderedRef.current = onRendered; }, [onRendered]);

  useEffect(() => {
    let active = true;
    let renderTask: RenderTask | null = null;
    let textLayer: TextLayer | null = null;
    let deferredFrame: number | null = null;
    const canvas = canvasRef.current;
    const textContainer = textLayerRef.current;
    if (!canvas || !textContainer) return;
    setRenderError(null);
    const render = () => { void document.getPage(pageIndex + 1).then(async (page) => {
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
      const allocation = safeCanvasAllocation(
        viewport.width,
        viewport.height,
        window.devicePixelRatio || 1,
        PAGE_DPR_CAP,
      );
      if (!allocation) {
        canvas.width = 0;
        canvas.height = 0;
        textContainer.replaceChildren();
        setRenderError('too-large');
        return;
      }
      const { outputScale } = allocation;
      canvas.width = allocation.width;
      canvas.height = allocation.height;
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
        setRenderError('failed');
      }
    }); };
    if (visible) render();
    else deferredFrame = window.requestAnimationFrame(render);
    return () => {
      active = false;
      if (deferredFrame !== null) window.cancelAnimationFrame(deferredFrame);
      renderTask?.cancel();
      textLayer?.cancel();
      textContainer.replaceChildren();
      canvas.width = 0;
      canvas.height = 0;
    };
  }, [availableHeight, availableWidth, document, fitMode, pageIndex, searchQuery, viewRotation, visible, zoom]);

  return (
    <article
      className={`pdf-page-surface${selected ? ' selected' : ''}${searchHits > 0 ? ' search-result-page' : ''}`}
      style={{ width: dimensions.width, minHeight: dimensions.height }}
      data-page-index={pageIndex}
      data-visible={visible ? 'true' : 'false'}
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
      {renderError === 'too-large' && <div className="page-render-error" role="status">This page is too large to render safely.</div>}
      {renderError === 'failed' && <div className="page-render-error" role="status">Page {pageIndex + 1} could not be rendered.</div>}
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
  onReorder(pageIndex: number, direction: -1 | 1): void;
  onFocusPage(pageIndex: number): void;
}

export const PageThumbnail = memo(function PageThumbnail({
  document,
  pageIndex,
  pageCount,
  selected,
  current,
  onSelect,
  onNavigate,
  onReorder,
  onFocusPage,
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
      const allocation = safeCanvasAllocation(
        viewport.width,
        viewport.height,
        window.devicePixelRatio || 1,
        THUMBNAIL_DPR_CAP,
      );
      if (!allocation) return;
      const { outputScale } = allocation;
      canvas.width = allocation.width;
      canvas.height = allocation.height;
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
      onFocus={() => onFocusPage(pageIndex)}
      onClick={(event) => onSelect(pageIndex, event)}
      onKeyDown={(event) => {
        if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
          event.preventDefault();
          event.stopPropagation();
          onReorder(pageIndex, event.key === 'ArrowUp' ? -1 : 1);
          return;
        }
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
