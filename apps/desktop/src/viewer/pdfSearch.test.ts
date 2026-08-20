import { describe, expect, it, vi } from 'vitest';
import { PdfTextIndexer, normalizeSearchText } from './pdfSearch';

function documentWithPages(pages: string[], delayLast = false) {
  return {
    numPages: pages.length,
    getPage: vi.fn(async (pageNumber: number) => ({
      getTextContent: vi.fn(async () => {
        if (delayLast && pageNumber === pages.length) {
          await new Promise<void>((resolve) => setTimeout(resolve, 75));
        }
        return { items: pages[pageNumber - 1]
          ? [{ str: pages[pageNumber - 1] }]
          : [],
        };
      }),
    })),
  };
}

describe('ephemeral bounded PDF search', () => {
  it('normalizes Unicode, publishes early results, and finishes image-only pages', async () => {
    const document = documentWithPages(['Cafe\u0301 résumé', '', 'Last CAFÉ result'], true);
    const indexer = new PdfTextIndexer(document as never);
    indexer.setQuery('CAFÉ');
    await vi.waitFor(() => expect(indexer.getSnapshot().resultCount).toBeGreaterThan(0));
    expect(indexer.getSnapshot().stillSearching).toBe(true);
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
    const snapshot = indexer.getSnapshot();
    expect(snapshot.resultCount).toBe(2);
    expect([...snapshot.resultPages.keys()]).toEqual([0, 2]);
    expect(snapshot.imageOnlyPages).toBe(1);
    indexer.destroy();
  });

  it('cancels the prior query generation and keeps no persistent index', async () => {
    const document = documentWithPages(['alpha', 'beta', 'alphabet']);
    const indexer = new PdfTextIndexer(document as never);
    indexer.setQuery('alpha');
    indexer.setQuery('beta');
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
    expect(indexer.getSnapshot().query).toBe('beta');
    expect(indexer.getSnapshot().resultCount).toBe(1);
    indexer.destroy();
    expect(indexer.getSnapshot().totalPages).toBe(3);
  });

  it('uses NFKC case-insensitive normalization without claiming OCR', () => {
    expect(normalizeSearchText('Ｆｉｌｅ Café')).toBe('file café');
  });
});
