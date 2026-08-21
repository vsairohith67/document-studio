import { describe, expect, it } from 'vitest';
import { reorderSelectedPages, selectCurrentVisiblePage } from './ViewerWorkspace';

describe('viewer organizer and visibility invariants', () => {
  it('moves contiguous and non-contiguous selections as one stable group', () => {
    expect(reorderSelectedPages([0, 1, 2, 3, 4], new Set([1, 3]), 3, -1).order)
      .toEqual([1, 3, 0, 2, 4]);
    expect(reorderSelectedPages([0, 1, 2, 3, 4], new Set([1, 2]), 1, 1).order)
      .toEqual([0, 3, 1, 2, 4]);
  });

  it('uses the focused page alone when it is not selected and preserves boundaries', () => {
    const moved = reorderSelectedPages([0, 1, 2], new Set([0]), 1, 1);
    expect(moved.order).toEqual([0, 2, 1]);
    expect([...moved.selected]).toEqual([1]);
    expect(reorderSelectedPages([0, 1], new Set([0]), 0, -1).moved).toBe(false);
    expect(reorderSelectedPages([0, 1], new Set([1]), 1, 1).moved).toBe(false);
  });

  it('chooses current page by visible area, top distance and visual index only', () => {
    expect(selectCurrentVisiblePage([], 7)).toBe(7);
    expect(selectCurrentVisiblePage([
      { sourcePageIndex: 4, visualIndex: 3, intersectionArea: 500, viewportTopDistance: 60 },
      { sourcePageIndex: 2, visualIndex: 2, intersectionArea: 900, viewportTopDistance: 100 },
      { sourcePageIndex: 1, visualIndex: 1, intersectionArea: 900, viewportTopDistance: 40 },
    ], 0)).toBe(1);
  });
});
