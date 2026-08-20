import type { PDFDocumentProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';

const MAX_CONCURRENT_EXTRACTIONS = 2;
const MAX_PAGE_CHARACTERS = 1_000_000;
const MAX_PAGE_ITEMS = 100_000;
const MAX_CACHED_CHARACTERS = 16_000_000;
const MAX_RECORDED_RESULTS = 100_000;

export interface SearchSnapshot {
  query: string;
  resultPages: ReadonlyMap<number, readonly number[]>;
  resultCount: number;
  searchedPages: number;
  totalPages: number;
  stillSearching: boolean;
  limited: boolean;
  imageOnlyPages: number;
}

const EMPTY_SNAPSHOT: SearchSnapshot = {
  query: '',
  resultPages: new Map(),
  resultCount: 0,
  searchedPages: 0,
  totalPages: 0,
  stillSearching: false,
  limited: false,
  imageOnlyPages: 0,
};

export function normalizeSearchText(value: string): string {
  return value.normalize('NFKC').toLocaleLowerCase();
}

export class PdfTextIndexer {
  private document: PDFDocumentProxy | null;
  private readonly listeners = new Set<() => void>();
  private readonly textCache = new Map<number, string>();
  private readonly imageOnly = new Set<number>();
  private readonly unavailable = new Set<number>();
  private queue: number[] = [];
  private queued = new Set<number>();
  private readonly inFlight = new Set<number>();
  private active = 0;
  private cachedCharacters = 0;
  private searched = new Set<number>();
  private resultPages = new Map<number, readonly number[]>();
  private query = '';
  private resultCount = 0;
  private limited = false;
  private snapshot: SearchSnapshot;

  constructor(document: PDFDocumentProxy) {
    this.document = document;
    this.snapshot = { ...EMPTY_SNAPSHOT, totalPages: document.numPages };
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): SearchSnapshot => this.snapshot;

  prioritize(pageIndexes: readonly number[]): void {
    for (const pageIndex of [...pageIndexes].reverse()) {
      if (pageIndex < 0 || pageIndex >= this.snapshot.totalPages
        || this.textCache.has(pageIndex) || this.unavailable.has(pageIndex)
        || this.inFlight.has(pageIndex)) continue;
      if (this.queued.has(pageIndex)) {
        this.queue = this.queue.filter((entry) => entry !== pageIndex);
      }
      this.queue.unshift(pageIndex);
      this.queued.add(pageIndex);
    }
    this.pump();
  }

  setQuery(value: string): void {
    this.query = normalizeSearchText(value.trim());
    this.resultPages = new Map();
    this.resultCount = 0;
    this.searched = new Set();
    this.limited = false;
    if (!this.query) {
      this.queue = [];
      this.queued.clear();
      this.publish();
      return;
    }
    for (let pageIndex = 0; pageIndex < this.snapshot.totalPages; pageIndex += 1) {
      const text = this.textCache.get(pageIndex);
      if (text !== undefined) this.searchPage(pageIndex, text);
      else if (this.unavailable.has(pageIndex)) this.searched.add(pageIndex);
      else if (!this.inFlight.has(pageIndex)) this.enqueue(pageIndex);
    }
    this.publish();
    this.pump();
  }

  destroy(): void {
    this.document = null;
    this.queue = [];
    this.queued.clear();
    this.inFlight.clear();
    this.textCache.clear();
    this.listeners.clear();
  }

  private enqueue(pageIndex: number): void {
    if (pageIndex < 0 || pageIndex >= this.snapshot.totalPages
      || this.queued.has(pageIndex) || this.inFlight.has(pageIndex)
      || this.textCache.has(pageIndex) || this.unavailable.has(pageIndex)) return;
    this.queue.push(pageIndex);
    this.queued.add(pageIndex);
  }

  private pump(): void {
    while (this.document && this.active < MAX_CONCURRENT_EXTRACTIONS && this.queue.length > 0) {
      const pageIndex = this.queue.shift();
      if (pageIndex === undefined) return;
      this.queued.delete(pageIndex);
      if (pageIndex < 0 || pageIndex >= this.snapshot.totalPages
        || this.textCache.has(pageIndex) || this.unavailable.has(pageIndex)
        || this.inFlight.has(pageIndex)) continue;
      this.inFlight.add(pageIndex);
      this.active += 1;
      void this.extract(pageIndex).then((text) => {
        if (!this.document) return;
        if (text === null) {
          this.unavailable.add(pageIndex);
          if (this.query) this.markUnavailableSearched(pageIndex);
        } else {
          this.cache(pageIndex, text);
          if (text.length === 0) this.imageOnly.add(pageIndex);
          if (this.query) this.searchPage(pageIndex, text);
        }
      }).catch(() => {
        if (this.document) {
          this.unavailable.add(pageIndex);
          if (this.query) this.markUnavailableSearched(pageIndex);
        }
      }).finally(() => {
        this.inFlight.delete(pageIndex);
        this.active = Math.max(0, this.active - 1);
        if (this.document) {
          this.publish();
          window.setTimeout(() => this.pump(), 0);
        }
      });
    }
  }

  private async extract(pageIndex: number): Promise<string | null> {
    const document = this.document;
    if (!document) return null;
    const page = await document.getPage(pageIndex + 1);
    const content = await page.getTextContent({ disableNormalization: false });
    if (content.items.length > MAX_PAGE_ITEMS) return null;
    let text = '';
    for (const item of content.items) {
      if (!('str' in item)) continue;
      text += `${item.str} `;
      if (text.length > MAX_PAGE_CHARACTERS) return null;
    }
    return normalizeSearchText(text);
  }

  private cache(pageIndex: number, text: string): void {
    const previous = this.textCache.get(pageIndex);
    if (previous !== undefined) this.cachedCharacters -= previous.length;
    this.textCache.delete(pageIndex);
    this.textCache.set(pageIndex, text);
    this.cachedCharacters += text.length;
    while (this.cachedCharacters > MAX_CACHED_CHARACTERS && this.textCache.size > 1) {
      const oldest = this.textCache.entries().next().value as [number, string] | undefined;
      if (!oldest) break;
      this.textCache.delete(oldest[0]);
      this.cachedCharacters -= oldest[1].length;
    }
  }

  private searchPage(pageIndex: number, text: string): void {
    this.searched.add(pageIndex);
    this.replacePageResults(pageIndex, []);
    if (!this.query || this.resultCount >= MAX_RECORDED_RESULTS) {
      if (this.query && this.resultCount >= MAX_RECORDED_RESULTS) this.limited = true;
      return;
    }
    const offsets: number[] = [];
    let offset = 0;
    while (offset <= text.length - this.query.length) {
      const found = text.indexOf(this.query, offset);
      if (found < 0) break;
      if (this.resultCount + offsets.length >= MAX_RECORDED_RESULTS) {
        this.limited = true;
        break;
      }
      offsets.push(found);
      offset = found + Math.max(1, this.query.length);
    }
    this.replacePageResults(pageIndex, offsets);
  }

  private markUnavailableSearched(pageIndex: number): void {
    this.searched.add(pageIndex);
    this.replacePageResults(pageIndex, []);
  }

  private replacePageResults(pageIndex: number, offsets: readonly number[]): void {
    const previous = this.resultPages.get(pageIndex);
    if (previous) this.resultCount = Math.max(0, this.resultCount - previous.length);
    this.resultPages.delete(pageIndex);
    if (offsets.length > 0) {
      this.resultPages.set(pageIndex, offsets);
      this.resultCount += offsets.length;
    }
  }

  private publish(): void {
    this.snapshot = {
      query: this.query,
      resultPages: new Map(this.resultPages),
      resultCount: this.resultCount,
      searchedPages: this.searched.size,
      totalPages: this.snapshot.totalPages,
      stillSearching: Boolean(this.query) && this.searched.size < this.snapshot.totalPages,
      limited: this.limited,
      imageOnlyPages: this.imageOnly.size,
    };
    for (const listener of this.listeners) listener();
  }
}
