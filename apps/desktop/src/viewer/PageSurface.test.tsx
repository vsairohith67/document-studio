import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import {
  MAX_CANVAS_HEIGHT,
  MAX_CANVAS_PIXELS,
  MAX_CANVAS_WIDTH,
  PAGE_DPR_CAP,
} from './pdfSession';
import { PageSurface, safeCanvasAllocation } from './PageSurface';

describe('fail-closed PDF canvas allocation', () => {
  it('enforces finite per-axis, pixel and CSS limits after integer rounding', () => {
    expect(safeCanvasAllocation(8192, 2048, 1, PAGE_DPR_CAP)).toEqual({
      width: MAX_CANVAS_WIDTH,
      height: 2048,
      outputScale: 1,
    });
    expect(safeCanvasAllocation(2048, 2048, 2, PAGE_DPR_CAP)).toEqual({
      width: 4096, height: 4096, outputScale: 2,
    });
    expect(8192 * 4096).toBeGreaterThan(MAX_CANVAS_PIXELS);
    expect(safeCanvasAllocation(8192, 4096, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(4096, 4097, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(MAX_CANVAS_WIDTH + 1, 1, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(1, MAX_CANVAS_HEIGHT + 1, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(1, 1_000_000, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(Number.POSITIVE_INFINITY, 10, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(10, Number.NaN, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(0, 10, 1, PAGE_DPR_CAP)).toBeNull();
    expect(safeCanvasAllocation(1, 1, Number.POSITIVE_INFINITY, PAGE_DPR_CAP)).toEqual({
      width: 1, height: 1, outputScale: 1,
    });
  });

  it('does not allocate, render or announce completion for an extreme page', async () => {
    const renderPage = vi.fn();
    const onRendered = vi.fn();
    const page = {
      getViewport: vi.fn(() => ({ width: 100_000, height: 10 })),
      render: renderPage,
      getTextContent: vi.fn(),
    };
    const pdf = { numPages: 1, getPage: vi.fn(async () => page) };
    render(<PageSurface
      document={pdf as never}
      pageIndex={0}
      pageCount={1}
      zoom={1}
      fitMode="actual"
      availableWidth={900}
      availableHeight={700}
      viewRotation={0}
      selected={false}
      searchQuery=""
      searchHits={0}
      visible
      onSelect={vi.fn()}
      onRendered={onRendered}
    />);
    expect((await screen.findByRole('status')).textContent).toBe('This page is too large to render safely.');
    expect(renderPage).not.toHaveBeenCalled();
    expect(page.getTextContent).not.toHaveBeenCalled();
    expect(onRendered).not.toHaveBeenCalled();
    const canvas = screen.getByLabelText('Rendered page 1') as HTMLCanvasElement;
    expect(canvas.width).toBe(0);
    expect(canvas.height).toBe(0);
  });
});
