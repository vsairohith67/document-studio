import { describe, expect, it, vi } from 'vitest';
import { PdfTextIndexer, normalizeSearchText } from './pdfSearch';

interface DeferredTextContent {
  promise: Promise<{ items: Array<{ str: string }> }>;
  resolve: (value: { items: Array<{ str: string }> }) => void;
  reject: (reason: Error) => void;
}

function deferredTextContent(): DeferredTextContent {
  let resolve!: DeferredTextContent['resolve'];
  let reject!: DeferredTextContent['reject'];
  const promise = new Promise<{ items: Array<{ str: string }> }>((complete, fail) => {
    resolve = complete;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function controlledDocument(pageCount: number) {
  const pending = Array.from({ length: pageCount }, () => [] as DeferredTextContent[]);
  const getPage = vi.fn(async (pageNumber: number) => ({
    getTextContent: vi.fn(() => {
      const deferred = deferredTextContent();
      pending[pageNumber - 1].push(deferred);
      return deferred.promise;
    }),
  }));
  return { document: { numPages: pageCount, getPage }, getPage, pending };
}

function release(deferred: DeferredTextContent, text: string): void {
  deferred.resolve({ items: text ? [{ str: text }] : [] });
}

function mappedResultCount(indexer: PdfTextIndexer): number {
  return [...indexer.getSnapshot().resultPages.values()]
    .reduce((total, offsets) => total + offsets.length, 0);
}

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
  it('does not duplicate active extraction or inflate per-page result accounting', async () => {
    const controlled = controlledDocument(3);
    const indexer = new PdfTextIndexer(controlled.document as never);

    indexer.prioritize([0]);
    await vi.waitFor(() => expect(controlled.pending[0]).toHaveLength(1));
    for (let attempt = 0; attempt < 5; attempt += 1) indexer.prioritize([0]);
    indexer.setQuery('needle');
    for (let attempt = 0; attempt < 5; attempt += 1) indexer.prioritize([0]);
    expect(controlled.getPage.mock.calls.filter(([pageNumber]) => pageNumber === 1)).toHaveLength(1);

    release(controlled.pending[0][0], 'needle');
    await vi.waitFor(() => expect(indexer.getSnapshot().resultCount).toBe(1));

    const snapshot = indexer.getSnapshot();
    expect(snapshot.resultPages.get(0)).toEqual([0]);
    expect(snapshot.resultCount).toBe(mappedResultCount(indexer));
    expect(snapshot.resultCount).toBe(1);
    expect(controlled.getPage.mock.calls.filter(([pageNumber]) => pageNumber === 1)).toHaveLength(1);
    indexer.destroy();
  });

  it('keeps only the latest query while multiple extractions are in flight', async () => {
    const controlled = controlledDocument(3);
    const indexer = new PdfTextIndexer(controlled.document as never);

    indexer.setQuery('alpha');
    await vi.waitFor(() => {
      expect(controlled.pending[0]).toHaveLength(1);
      expect(controlled.pending[1]).toHaveLength(1);
    });
    indexer.setQuery('beta');
    release(controlled.pending[0][0], 'alpha');
    release(controlled.pending[1][0], 'beta');
    await vi.waitFor(() => expect(controlled.pending[2]).toHaveLength(1));
    release(controlled.pending[2][0], 'alphabet');
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));

    const snapshot = indexer.getSnapshot();
    expect(snapshot.query).toBe('beta');
    expect([...snapshot.resultPages.keys()]).toEqual([1]);
    expect(snapshot.resultCount).toBe(mappedResultCount(indexer));
    expect(snapshot.resultCount).toBe(1);
    indexer.destroy();
  });

  it('replaces cached page results idempotently across repeated queries', async () => {
    const document = documentWithPages(['needle']);
    const indexer = new PdfTextIndexer(document as never);
    for (const query of ['needle', 'need', 'needle', 'needle']) {
      indexer.setQuery(query);
      await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
      expect(indexer.getSnapshot().resultPages.get(0)).toEqual([0]);
      expect(indexer.getSnapshot().resultCount).toBe(1);
      expect(indexer.getSnapshot().resultCount).toBe(mappedResultCount(indexer));
    }
    expect(document.getPage).toHaveBeenCalledTimes(1);
    indexer.destroy();
  });

  it('deduplicates one hundred repeated visible-page priorities', async () => {
    const controlled = controlledDocument(3);
    const indexer = new PdfTextIndexer(controlled.document as never);
    indexer.prioritize([0]);
    await vi.waitFor(() => expect(controlled.pending[0]).toHaveLength(1));
    for (let attempt = 0; attempt < 100; attempt += 1) indexer.prioritize([0, 0, 0]);
    indexer.setQuery('needle');
    for (let attempt = 0; attempt < 100; attempt += 1) indexer.prioritize([0, 0]);
    expect(controlled.getPage.mock.calls.filter(([pageNumber]) => pageNumber === 1)).toHaveLength(1);
    release(controlled.pending[0][0], 'needle');
    await vi.waitFor(() => expect(indexer.getSnapshot().resultCount).toBe(1));
    expect(indexer.getSnapshot().resultCount).toBe(mappedResultCount(indexer));
    expect(controlled.getPage.mock.calls.filter(([pageNumber]) => pageNumber === 1)).toHaveLength(1);
    indexer.destroy();
  });

  it('finishes image-only and unavailable pages without inventing results', async () => {
    const controlled = controlledDocument(3);
    const indexer = new PdfTextIndexer(controlled.document as never);
    indexer.setQuery('missing');
    await vi.waitFor(() => {
      expect(controlled.pending[0]).toHaveLength(1);
      expect(controlled.pending[1]).toHaveLength(1);
    });
    release(controlled.pending[0][0], '');
    controlled.pending[1][0].reject(new Error('page unavailable'));
    await vi.waitFor(() => expect(controlled.pending[2]).toHaveLength(1));
    release(controlled.pending[2][0], 'other text');
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
    expect(indexer.getSnapshot()).toMatchObject({
      resultCount: 0,
      searchedPages: 3,
      imageOnlyPages: 1,
      limited: false,
    });
    expect(mappedResultCount(indexer)).toBe(0);
    indexer.destroy();
  });

  it('keeps result-limit accounting truthful when cached pages are replaced', async () => {
    const document = documentWithPages(['a'.repeat(100_001), 'a']);
    const indexer = new PdfTextIndexer(document as never);
    indexer.setQuery('a');
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
    expect(indexer.getSnapshot().resultCount).toBe(100_000);
    expect(indexer.getSnapshot().limited).toBe(true);
    expect(indexer.getSnapshot().resultCount).toBe(mappedResultCount(indexer));

    indexer.setQuery('aa');
    expect(indexer.getSnapshot().resultCount).toBe(50_000);
    expect(indexer.getSnapshot().limited).toBe(false);
    expect(indexer.getSnapshot().resultCount).toBe(mappedResultCount(indexer));
    expect(document.getPage).toHaveBeenCalledTimes(2);
    indexer.destroy();
  });

  it('publishes nothing after destruction during extraction', async () => {
    const controlled = controlledDocument(1);
    const indexer = new PdfTextIndexer(controlled.document as never);
    const listener = vi.fn();
    indexer.subscribe(listener);
    indexer.setQuery('needle');
    await vi.waitFor(() => expect(controlled.pending[0]).toHaveLength(1));
    const snapshotBeforeDestroy = indexer.getSnapshot();
    const callsBeforeDestroy = listener.mock.calls.length;
    indexer.destroy();
    release(controlled.pending[0][0], 'needle');
    await new Promise<void>((resolve) => window.setTimeout(resolve, 0));
    expect(listener).toHaveBeenCalledTimes(callsBeforeDestroy);
    expect(indexer.getSnapshot()).toBe(snapshotBeforeDestroy);
  });

  it.each([17, 41, 97])('keeps seeded completion order %i deterministic', async (seed) => {
    const controlled = controlledDocument(4);
    const indexer = new PdfTextIndexer(controlled.document as never);
    const released = new Set<DeferredTextContent>();
    let state = seed;
    const nextRandom = () => {
      state = (state * 48_271) % 2_147_483_647;
      return state / 2_147_483_647;
    };
    indexer.setQuery('target');
    for (let releaseCount = 0; releaseCount < 4; releaseCount += 1) {
      let available: Array<{ pageIndex: number; deferred: DeferredTextContent }> = [];
      await vi.waitFor(() => {
        available = controlled.pending.flatMap((entries, pageIndex) => entries
          .filter((entry) => !released.has(entry))
          .map((deferred) => ({ pageIndex, deferred })));
        expect(available.length).toBeGreaterThan(0);
      });
      const selected = available[Math.floor(nextRandom() * available.length)];
      released.add(selected.deferred);
      release(selected.deferred, selected.pageIndex === 2 ? 'target' : `page ${selected.pageIndex}`);
    }
    await vi.waitFor(() => expect(indexer.getSnapshot().stillSearching).toBe(false));
    expect([...indexer.getSnapshot().resultPages.keys()]).toEqual([2]);
    expect(indexer.getSnapshot().resultCount).toBe(1);
    expect(indexer.getSnapshot().resultCount).toBe(mappedResultCount(indexer));
    indexer.destroy();
  });

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
