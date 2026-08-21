import { expect, test, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

declare global {
  interface Window {
    __G03_TEST_EVIDENCE__: {
      rangeCalls: Array<{ begin: number; end: number; sessionId: string; generation: number }>;
      peakReads: number;
      activeReads: number;
      closed: boolean;
      dropEnabled: boolean;
      plans: unknown[];
      pageCount: number;
      fixtureBytes: number;
      openCount: number;
      closeCount: number;
      activeSessions: number;
      candidateSessions: number;
      maxActiveSessions: number;
      closedSessionIds: string[];
    };
    __G03_PRINT_CALLED__: boolean;
  }
}

function syntheticPdf(
  pageCount: number,
  includeText = true,
  dimensions?: Array<{ width: number; height: number; userUnit?: number }>,
): Uint8Array {
  const encoder = new TextEncoder();
  const objects: string[] = [];
  const pageReferences = Array.from({ length: pageCount }, (_, index) => `${4 + index * 2} 0 R`).join(' ');
  objects.push('<< /Type /Catalog /Pages 2 0 R /OpenAction << /S /JavaScript /JS (this.print\\(\\)) >> >>');
  objects.push(`<< /Type /Pages /Count ${pageCount} /Kids [${pageReferences}] >>`);
  objects.push('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
  for (let index = 0; index < pageCount; index += 1) {
    const pageObject = 4 + index * 2;
    const streamObject = pageObject + 1;
    const width = dimensions?.[index]?.width ?? (index % 3 === 0 ? 612 : index % 3 === 1 ? 792 : 540);
    const height = dimensions?.[index]?.height ?? (index % 3 === 1 ? 612 : 792);
    const userUnit = dimensions?.[index]?.userUnit ? ` /UserUnit ${dimensions[index].userUnit}` : '';
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${width} ${height}]${userUnit} /Resources << /Font << /F1 3 0 R >> >> /Contents ${streamObject} 0 R /Annots [<< /Type /Annot /Subtype /Link /Rect [0 0 100 20] /A << /S /URI /URI (https://example.com/) >> >>] >>`);
    const stream = includeText
      ? `BT /F1 14 Tf 72 72 Td (Page ${index + 1} unique-text-${index + 1}) Tj ET\n`
      : '0 0 100 100 re S\n';
    objects.push(`<< /Length ${encoder.encode(stream).length} >>\nstream\n${stream}endstream`);
  }
  const chunks: string[] = ['%PDF-1.7\n%G03\n'];
  const offsets: number[] = [];
  let length = encoder.encode(chunks[0]).length;
  objects.forEach((object, index) => {
    offsets.push(length);
    const chunk = `${index + 1} 0 obj\n${object}\nendobj\n`;
    chunks.push(chunk);
    length += encoder.encode(chunk).length;
  });
  const xref = length;
  chunks.push(`xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`);
  for (const offset of offsets) chunks.push(`${String(offset).padStart(10, '0')} 00000 n \n`);
  chunks.push(`trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`);
  return encoder.encode(chunks.join(''));
}

async function installTransport(page: Page, pageCount: number, suppliedBytes = syntheticPdf(pageCount)) {
  await installTransportSequence(page, [{
    pageCount,
    bytes: suppliedBytes,
    displayName: `fixture-${pageCount}-pages.pdf`,
  }]);
}

interface BrowserFixture {
  pageCount: number;
  bytes: Uint8Array;
  displayName: string;
  readDelayMs?: number;
}

async function installTransportSequence(page: Page, suppliedFixtures: BrowserFixture[]) {
  await page.addInitScript((serializedFixtures) => {
    const fixtures = serializedFixtures.map((fixture) => ({
      ...fixture,
      bytes: Uint8Array.from(fixture.bytes),
    }));
    const evidence = window.__G03_TEST_EVIDENCE__ = {
      rangeCalls: [], peakReads: 0, activeReads: 0, closed: false,
      dropEnabled: false, plans: [], pageCount: fixtures[0].pageCount,
      fixtureBytes: fixtures[0].bytes.byteLength, openCount: 0, closeCount: 0,
      activeSessions: 0, candidateSessions: 0, maxActiveSessions: 0, closedSessionIds: [],
    };
    const sessions = new Map<string, { bytes: Uint8Array; generation: number; delay: number }>();
    let fixtureIndex = 0;
    const makeJob = (request: { plan: { operationId: string }; viewerSessionId: string }) => ({
      id: 'browser-job-0001', operationId: request.plan.operationId, operationVersion: '1.0.0',
      state: 'queued', stage: null, sequence: 0,
      progress: { completedUnits: 0, totalUnits: 0, unit: 'steps' },
      destinationDirectory: 'opaque-destination', requestedOutputName: 'planned.pdf',
      resolvedOutputName: null, cancellationRequestedAt: null,
      createdAt: '2026-08-17T00:00:00Z', updatedAt: '2026-08-17T00:00:00Z',
      finishedAt: null, version: 0, inputs: [], outputs: [], errors: [],
    });
    globalThis.__DOCUMENT_STUDIO_G03_TEST_TRANSPORT__ = {
      async open() {
        const fixture = fixtures[Math.min(fixtureIndex, fixtures.length - 1)];
        const ordinal = fixtureIndex;
        fixtureIndex += 1;
        evidence.openCount += 1;
        const sessionId = fixtures.length === 1 ? 'browser-opaque-session' : `browser-opaque-session-${ordinal + 1}`;
        const generation = 41 + ordinal;
        sessions.set(sessionId, { bytes: fixture.bytes, generation, delay: fixture.readDelayMs ?? 2 });
        evidence.closed = false;
        evidence.activeSessions = sessions.size;
        evidence.candidateSessions = Math.max(0, sessions.size - 1);
        evidence.maxActiveSessions = Math.max(evidence.maxActiveSessions, sessions.size);
        return {
          sessionId, generation, displayName: fixture.displayName,
          sizeBytes: fixture.bytes.byteLength, modifiedAt: '2026-08-17T00:00:00Z',
          mimeType: 'application/pdf', fileIdentity: `browser-opaque-identity-${ordinal + 1}`,
        };
      },
      async readRange(request: { sessionId: string; generation: number; begin: number; end: number }) {
        const session = sessions.get(request.sessionId);
        if (!session || request.generation !== session.generation) throw new Error('expired session');
        if (request.begin < 0 || request.end <= request.begin || request.end > session.bytes.byteLength || request.end - request.begin > 1024 * 1024) throw new Error('invalid range');
        evidence.rangeCalls.push({ ...request });
        evidence.activeReads += 1;
        evidence.peakReads = Math.max(evidence.peakReads, evidence.activeReads);
        await new Promise((resolve) => setTimeout(resolve, session.delay));
        evidence.activeReads -= 1;
        return session.bytes.slice(request.begin, request.end);
      },
      async close(request: { sessionId: string; generation: number }) {
        const session = sessions.get(request.sessionId);
        if (session?.generation === request.generation) {
          sessions.delete(request.sessionId);
          evidence.closeCount += 1;
          evidence.closedSessionIds.push(request.sessionId);
        }
        evidence.activeSessions = sessions.size;
        evidence.candidateSessions = Math.max(0, sessions.size - 1);
        evidence.closed = sessions.size === 0;
      },
      async setDropEnabled(enabled: boolean) { evidence.dropEnabled = enabled; },
      async chooseDestination() { return { grantId: 'opaque-destination-grant', displayName: 'Test outputs' }; },
      async revokeDestination() {},
      async createCorePdf(request: unknown) { evidence.plans.push(request); return makeJob(request as never); },
      async onProgress() { return () => undefined; },
    };
  }, suppliedFixtures.map((fixture) => ({ ...fixture, bytes: Array.from(fixture.bytes) })));
}

async function openViewer(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Viewer', exact: true }).click();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.locator('.pdf-page-surface canvas').first()).toBeVisible();
  await expect.poll(() => page.locator('.pdf-page-surface canvas').first().evaluate((canvas) => (canvas as HTMLCanvasElement).width)).toBeGreaterThan(0);
}

test('renders progressively through the matching worker and bounded raw range contract', async ({ page }) => {
  await installTransport(page, 1000);
  await page.addInitScript(() => {
    window.__G03_PRINT_CALLED__ = false;
    window.print = () => { window.__G03_PRINT_CALLED__ = true; };
  });
  const popup = page.waitForEvent('popup', { timeout: 500 }).catch(() => null);
  await openViewer(page);
  await expect(page.getByRole('button', { name: 'Page 1 of 1000' }).first()).toBeVisible();
  const evidence = await page.evaluate(() => window.__G03_TEST_EVIDENCE__);
  expect(evidence.rangeCalls.length).toBeGreaterThan(0);
  expect(evidence.rangeCalls.every((call) => call.end - call.begin <= 256 * 1024)).toBe(true);
  expect(evidence.rangeCalls.every((call) => !('path' in call))).toBe(true);
  expect(evidence.peakReads).toBeLessThanOrEqual(4);
  expect(await popup).toBeNull();
  expect(page.url()).toBe('http://127.0.0.1:1420/');
  const marks = await page.evaluate(() => ({
    page: performance.getEntriesByName('g03-first-page-displayed').at(-1)?.startTime,
    thumbnail: performance.getEntriesByName('g03-first-thumbnail-displayed').at(-1)?.startTime,
  }));
  expect(marks.page).toBeDefined();
  expect(marks.thumbnail).toBeDefined();
  expect(marks.page!).toBeLessThanOrEqual(marks.thumbnail!);
  expect(await page.locator('.pdf-page-surface').count()).toBeLessThanOrEqual(6);
  expect(await page.locator('.page-thumbnail').count()).toBeLessThanOrEqual(20);
  expect(await page.locator('a[href]').count()).toBe(0);
  expect(await page.evaluate(() => window.__G03_PRINT_CALLED__)).toBe(false);
  const selectedText = await page.locator('.textLayer span').first().evaluate((element) => {
    const selection = document.getSelection();
    const range = document.createRange();
    range.selectNodeContents(element);
    selection?.removeAllRanges();
    selection?.addRange(range);
    return selection?.toString() ?? '';
  });
  expect(selectedText).toContain('Page 1');
});

test('G04A lossless recompression renders the same representative PDF.js pixels', async ({ page }, testInfo) => {
  const sourcePath = testInfo.outputPath('g04a-visual-source.pdf');
  const compressedPath = testInfo.outputPath('g04a-visual-compressed.pdf');
  const sourceBytes = syntheticPdf(1);
  await writeFile(sourcePath, sourceBytes);
  execFileSync(
    resolve(import.meta.dirname, '..', 'src-tauri', 'resources', 'qpdf', '12.3.2', 'bin', 'qpdf.exe'),
    [
      sourcePath,
      '--stream-data=compress',
      '--object-streams=generate',
      '--recompress-flate',
      '--compression-level=9',
      compressedPath,
    ],
    { stdio: 'pipe' },
  );
  await installTransportSequence(page, [
    { pageCount: 1, bytes: sourceBytes, displayName: 'g04a-visual-source.pdf' },
    { pageCount: 1, bytes: await readFile(compressedPath), displayName: 'g04a-visual-compressed.pdf' },
  ]);
  await openViewer(page);
  await expect.poll(() => page.evaluate(() => performance
    .getEntriesByName('g03-first-page-displayed').length)).toBeGreaterThan(0);
  const completedRenders = await page.evaluate(() => performance
    .getEntriesByName('g03-first-page-displayed').length);
  const sourcePixels = await page.locator('.pdf-page-surface canvas').first()
    .evaluate((canvas) => (canvas as HTMLCanvasElement).toDataURL('image/png'));

  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByText('g04a-visual-compressed.pdf')).toBeVisible();
  await expect.poll(() => page.evaluate(() => performance
    .getEntriesByName('g03-first-page-displayed').length)).toBeGreaterThan(completedRenders);
  const compressedPixels = await page.locator('.pdf-page-surface canvas').first()
    .evaluate((canvas) => (canvas as HTMLCanvasElement).toDataURL('image/png'));
  expect(compressedPixels).toBe(sourcePixels);
});

test('unlocks an encrypted PDF only in memory and keeps structural operations disabled', async ({ page }, testInfo) => {
  const clearPath = testInfo.outputPath('clear.pdf');
  const encryptedPath = testInfo.outputPath('encrypted.pdf');
  await writeFile(clearPath, syntheticPdf(1));
  execFileSync(
    resolve(import.meta.dirname, '..', 'src-tauri', 'resources', 'qpdf', '12.3.2', 'bin', 'qpdf.exe'),
    [clearPath, '--encrypt', 'open-secret', 'test-owner', '256', '--', encryptedPath],
    { stdio: 'pipe' },
  );
  await installTransport(page, 1, await readFile(encryptedPath));

  await page.goto('/');
  await page.getByRole('button', { name: 'Viewer', exact: true }).click();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  const passwordDialog = page.getByRole('dialog', { name: 'Password required' });
  await expect(passwordDialog).toBeVisible();
  const passwordInput = page.getByRole('textbox', { name: 'Password' });
  await passwordInput.fill('open-secret');
  await page.getByRole('button', { name: 'Unlock in memory' }).click();

  await expect(passwordDialog).toBeHidden();
  await expect(page.locator('.pdf-page-surface canvas').first()).toBeVisible();
  await expect(page.getByText('Encrypted PDFs may be viewed with an in-memory password, but G03 output operations are unavailable.')).toBeVisible();
  await page.getByRole('button', { name: 'Choose destination' }).click();
  await expect(page.getByRole('button', { name: 'Apply / Export' })).toBeDisabled();
  expect(await page.evaluate(() => JSON.stringify(window.__G03_TEST_EVIDENCE__.plans))).not.toContain('open-secret');
});

test('virtualizes a 1000-page mixed-size document and searches before full indexing', async ({ page }) => {
  await installTransport(page, 1000);
  await openViewer(page);
  await page.keyboard.press('Control+f');
  const search = page.getByPlaceholder('Find in document');
  await search.fill('Page 2 unique-text-2');
  const searchStatus = page.locator('.search-bar [role="status"]');
  const nextResult = page.getByRole('button', { name: 'Next search result' });
  await expect(searchStatus).toContainText('still searching');
  try {
    await expect(searchStatus).toHaveText(/^1 of 1(?: · still searching)?$/, { timeout: 15_000 });
  } catch (error) {
    const diagnostic = {
      searchStatus: await searchStatus.textContent(),
      nextResultEnabled: await nextResult.isEnabled().catch(() => false),
      highlightedResultPages: await page.locator('.pdf-page-surface.search-result-page').count(),
    };
    throw new Error(`Incremental search readiness failed: ${JSON.stringify(diagnostic)}\n${error instanceof Error ? error.message : 'Search assertion failed.'}`);
  }
  await nextResult.click();
  const resultPage = page.locator('.pdf-page-surface.search-result-page[data-page-index="1"]');
  await expect(resultPage).toBeVisible();
  await expect(resultPage.locator('.textLayer')).toContainText('Page 2 unique-text-2');
  await page.getByRole('button', { name: 'Last page' }).click();
  await expect(page.locator('[data-page-index="999"]')).toBeAttached();
  await expect(page.locator('.virtual-page-stack [data-page-index="0"]')).toHaveCount(0);
  expect(await page.locator('.pdf-page-surface').count()).toBeLessThanOrEqual(6);
  expect(await page.locator('.page-thumbnail').count()).toBeLessThanOrEqual(20);
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.getByRole('button', { name: 'Fit width' }).click();
  await expect(page.locator('.pdf-page-surface canvas').first()).toBeVisible();
});

test('reports damaged PDF data without retaining a canvas', async ({ page }) => {
  await installTransport(page, 1, new TextEncoder().encode('%PDF-1.7\nthis is structurally damaged'));
  await page.goto('/');
  await page.getByRole('button', { name: 'Viewer', exact: true }).click();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByRole('alert')).toContainText('Document Studio could not complete that request.');
  await expect(page.locator('.pdf-page-surface canvas')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(0);
  expect(await page.evaluate(() => window.__G03_TEST_EVIDENCE__.closeCount)).toBe(1);
});

test('keeps active A through damaged B and commits valid C transactionally', async ({ page }) => {
  await installTransportSequence(page, [
    { pageCount: 2, bytes: syntheticPdf(2), displayName: 'active-a.pdf' },
    { pageCount: 1, bytes: new TextEncoder().encode('%PDF-1.7\nstructurally damaged'), displayName: 'damaged-b.pdf' },
    { pageCount: 3, bytes: syntheticPdf(3), displayName: 'replacement-c.pdf' },
  ]);
  await openViewer(page);
  await page.keyboard.press('Control+f');
  await page.getByPlaceholder('Find in document').fill('Page 1 unique-text-1');
  await expect(page.locator('.search-bar [role="status"]')).toContainText('1 of 1');
  const firstThumbnail = page.getByRole('button', { name: 'Page 1 of 2' }).first();
  await firstThumbnail.click();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.locator('.viewer-document-name')).toHaveText('active-a.pdf');
  await expect(firstThumbnail).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.search-bar [role="status"]')).toContainText('1 of 1');
  await expect(page.locator('.pdf-page-surface canvas').first()).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(1);
  expect(await page.evaluate(() => window.__G03_TEST_EVIDENCE__.closedSessionIds)).toContain('browser-opaque-session-2');

  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.locator('.viewer-document-name')).toHaveText('replacement-c.pdf');
  await expect(page.getByRole('button', { name: 'Page 1 of 3' }).first()).toBeVisible();
  await expect(page.locator('.search-bar')).toHaveCount(0);
  await expect(page.getByLabel('Pages and selection').getByText('0 selected')).toBeVisible();
  await expect(page.getByText('active-a.pdf')).toHaveCount(0);
  const replacementEvidence = await page.evaluate(() => window.__G03_TEST_EVIDENCE__);
  expect(replacementEvidence.activeSessions).toBe(1);
  expect(replacementEvidence.maxActiveSessions).toBe(2);
  expect(replacementEvidence.closeCount).toBe(2);
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(0);
  expect(await page.locator('.pdf-page-surface').count()).toBe(0);
});

test('candidate password Cancel preserves A without moving focus to Open', async ({ page }, testInfo) => {
  const clearPath = testInfo.outputPath('candidate-clear.pdf');
  const encryptedPath = testInfo.outputPath('candidate-encrypted.pdf');
  await writeFile(clearPath, syntheticPdf(1));
  execFileSync(
    resolve(import.meta.dirname, '..', 'src-tauri', 'resources', 'qpdf', '12.3.2', 'bin', 'qpdf.exe'),
    [clearPath, '--encrypt', 'candidate-secret', 'test-owner', '256', '--', encryptedPath],
    { stdio: 'pipe' },
  );
  await installTransportSequence(page, [
    { pageCount: 2, bytes: syntheticPdf(2), displayName: 'active-a.pdf' },
    { pageCount: 1, bytes: await readFile(encryptedPath), displayName: 'encrypted-b.pdf' },
  ]);
  await openViewer(page);
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByRole('dialog', { name: 'Password required' })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByRole('dialog', { name: 'Password required' })).toHaveCount(0);
  await expect(page.locator('.viewer-document-name')).toHaveText('active-a.pdf');
  await expect(page.locator('.pdf-page-surface canvas').first()).toBeVisible();
  await expect(page.getByRole('button', { name: 'Open PDF', exact: true }).first()).not.toBeFocused();
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(1);
  expect(await page.evaluate(() => window.__G03_TEST_EVIDENCE__.closeCount)).toBe(1);
});

test('slow B is disposed when C supersedes it and close disposes active plus candidate', async ({ page }) => {
  await installTransportSequence(page, [
    { pageCount: 2, bytes: syntheticPdf(2), displayName: 'active-a.pdf' },
    { pageCount: 2, bytes: syntheticPdf(2), displayName: 'slow-b.pdf', readDelayMs: 500 },
    { pageCount: 3, bytes: syntheticPdf(3), displayName: 'fast-c.pdf' },
    { pageCount: 2, bytes: syntheticPdf(2), displayName: 'slow-d.pdf', readDelayMs: 500 },
  ]);
  await openViewer(page);
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByText('Opening and validating the selected PDF…')).toBeVisible();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.locator('.viewer-document-name')).toHaveText('fast-c.pdf');
  await page.waitForTimeout(600);
  await expect(page.locator('.viewer-document-name')).toHaveText('fast-c.pdf');
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(1);
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await expect(page.getByText('Opening and validating the selected PDF…')).toBeVisible();
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.locator('.pdf-page-surface')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__G03_TEST_EVIDENCE__.activeSessions)).toBe(0);
  expect(await page.evaluate(() => window.__G03_TEST_EVIDENCE__.closeCount)).toBe(4);
});

test('rejects an extreme page without blocking normal neighboring pages', async ({ page }) => {
  const bytes = syntheticPdf(3, true, [
    { width: 612, height: 792 },
    { width: 10, height: 1_000_000, userUnit: 1000 },
    { width: 792, height: 612 },
  ]);
  await installTransport(page, 3, bytes);
  await openViewer(page);
  await page.getByRole('button', { name: 'Actual size' }).click();
  await page.getByRole('button', { name: 'Page 2 of 3' }).first().click();
  await page.keyboard.press('PageDown');
  await expect(page.getByText('This page is too large to render safely.')).toBeVisible();
  const extreme = page.locator('.pdf-page-surface[data-page-index="1"] canvas');
  expect(await extreme.evaluate((canvas) => ({ width: (canvas as HTMLCanvasElement).width, height: (canvas as HTMLCanvasElement).height })))
    .toEqual({ width: 0, height: 0 });
  await page.getByRole('button', { name: 'Last page' }).click();
  await expect.poll(() => page.locator('.pdf-page-surface[data-page-index="2"] canvas').evaluate((canvas) => (canvas as HTMLCanvasElement).width)).toBeGreaterThan(0);
});

test('truthfully reports that image-only pages have no searchable text', async ({ page }) => {
  await installTransport(page, 1, syntheticPdf(1, false));
  await openViewer(page);
  await page.keyboard.press('Control+f');
  await page.getByPlaceholder('Find in document').fill('not present');
  await expect(page.getByText('Searchable text is unavailable on image-only pages. OCR is a later goal.')).toBeVisible();
});

test('keeps organizer state ephemeral until Apply and emits a typed 0-based plan', async ({ page }) => {
  await installTransport(page, 12);
  await openViewer(page);
  expect(await page.evaluate(() => window.__G03_TEST_EVIDENCE__.plans)).toEqual([]);
  const first = page.getByRole('button', { name: 'Page 1 of 12' }).first();
  const second = page.getByRole('button', { name: 'Page 2 of 12' }).first();
  await first.click();
  await second.click({ modifiers: ['Control'] });
  await page.getByRole('button', { name: 'Choose destination' }).click();
  await page.getByRole('button', { name: 'Apply / Export' }).click();
  const plans = await page.evaluate(() => window.__G03_TEST_EVIDENCE__.plans);
  expect(plans).toHaveLength(1);
  expect(plans[0]).toMatchObject({
    viewerSessionId: 'browser-opaque-session', viewerGeneration: 41,
    destinationGrantId: 'opaque-destination-grant',
    plan: {
      schemaVersion: 1, operationId: 'pdf.extract-pages', sourcePageCount: 12,
      payload: { selectedPageIndexes: [0, 1] },
    },
  });
});

test('supports thumbnail keyboard navigation and preserves focus after accessible reorder', async ({ page }) => {
  await installTransport(page, 12);
  await openViewer(page);
  const second = page.getByRole('button', { name: 'Page 2 of 12' }).first();
  const third = page.getByRole('button', { name: 'Page 3 of 12' }).first();
  await second.click();
  await second.focus();
  await page.keyboard.press('ArrowDown');
  await expect(third).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(second).toBeFocused();
  await page.getByRole('button', { name: 'Move up' }).click();
  await expect(second).toBeFocused();
  await expect(page.locator('.sr-announcement')).toHaveText('Page 2 moved to position 1.');
  await page.getByRole('combobox', { name: /Operation/ }).selectOption('pdf.reorder-pages');
  await page.getByRole('button', { name: 'Choose destination' }).click();
  await page.getByRole('button', { name: 'Apply / Export' }).click();
  const plans = await page.evaluate(() => window.__G03_TEST_EVIDENCE__.plans);
  expect(plans[0]).toMatchObject({
    plan: {
      operationId: 'pdf.reorder-pages',
      payload: { orderedPageIndexes: [1, 0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] },
    },
  });
});

test('supports Alt+Arrow block reorder, boundaries, announcements and persisted visual order', async ({ page }) => {
  await installTransport(page, 8);
  await openViewer(page);
  const pageOne = page.getByRole('button', { name: 'Page 1 of 8' }).first();
  const pageTwo = page.getByRole('button', { name: 'Page 2 of 8' }).first();
  const pageThree = page.getByRole('button', { name: 'Page 3 of 8' }).first();
  const pageFour = page.getByRole('button', { name: 'Page 4 of 8' }).first();
  await pageOne.click();
  await pageOne.focus();
  await page.keyboard.press('Alt+ArrowUp');
  await expect(page.locator('.sr-announcement')).toHaveText('Selected pages are already at the beginning.');
  await expect(pageOne).toBeFocused();

  await pageTwo.click();
  await pageTwo.focus();
  await page.keyboard.press('Alt+ArrowUp');
  await expect(pageTwo).toBeFocused();
  await expect(page.locator('.sr-announcement')).toHaveText('Page 2 moved to position 1.');
  await page.keyboard.press('Alt+ArrowDown');
  await expect(pageTwo).toBeFocused();
  await expect(page.locator('.sr-announcement')).toHaveText('Page 2 moved to position 2.');

  await pageTwo.click();
  await pageThree.click({ modifiers: ['Control'] });
  await pageTwo.focus();
  await page.keyboard.press('Alt+ArrowUp');
  await expect(pageTwo).toBeFocused();
  await expect(page.locator('.sr-announcement')).toHaveText('2 selected pages moved to positions 1–2.');

  await pageOne.click();
  await pageFour.click({ modifiers: ['Control'] });
  await pageFour.focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect(pageFour).toBeFocused();
  await expect(page.locator('.sr-announcement')).toContainText('2 selected pages moved to positions');
  await page.keyboard.press('ArrowUp');
  await expect(pageFour).not.toBeFocused();

  const last = page.getByRole('button', { name: 'Page 8 of 8' }).first();
  await last.click();
  await last.focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect(last).toBeFocused();
  await expect(page.locator('.sr-announcement')).toHaveText('Selected pages are already at the end.');

  await page.getByRole('combobox', { name: /Operation/ }).selectOption('pdf.reorder-pages');
  await page.getByRole('button', { name: 'Choose destination' }).click();
  await page.getByRole('button', { name: 'Apply / Export' }).click();
  const planOrder = await page.evaluate(() => (
    window.__G03_TEST_EVIDENCE__.plans.at(-1) as { plan: { payload: { orderedPageIndexes: number[] } } }
  ).plan.payload.orderedPageIndexes);
  const visualOrder = await page.locator('.page-thumbnail').evaluateAll((buttons) => buttons
    .map((button) => Number((button as HTMLElement).dataset.pageIndex)));
  const persistedPositions = visualOrder.map((sourcePage) => planOrder.indexOf(sourcePage));
  expect(persistedPositions).toEqual([...persistedPositions].sort((left, right) => left - right));
});

test('tracks true viewport visibility instead of the TanStack overscan range', async ({ page }) => {
  await installTransport(page, 40);
  await openViewer(page);
  await page.getByRole('button', { name: 'Page 10 of 40' }).first().click();
  await page.locator('.page-canvas-scroll').evaluate((element) => {
    element.scrollTop = 8_000;
  });
  await expect.poll(() => page.locator('.virtual-page-row[data-visible="true"]').count()).toBeGreaterThan(0);
  await expect.poll(() => page.locator('.virtual-page-row[data-visible="false"]').count()).toBeGreaterThan(0);
  const expectedCurrent = await page.locator('.page-canvas-scroll').evaluate((root) => {
    const rootRect = root.getBoundingClientRect();
    return [...root.querySelectorAll<HTMLElement>('.virtual-page-row')]
      .map((row) => {
        const rect = row.getBoundingClientRect();
        const width = Math.max(0, Math.min(rect.right, rootRect.right) - Math.max(rect.left, rootRect.left));
        const height = Math.max(0, Math.min(rect.bottom, rootRect.bottom) - Math.max(rect.top, rootRect.top));
        return {
          page: Number(row.dataset.sourcePageIndex),
          visual: Number(row.dataset.visualIndex),
          area: width * height,
          top: Math.abs(rect.top - rootRect.top),
        };
      })
      .filter((entry) => entry.area > 0)
      .sort((left, right) => right.area - left.area || left.top - right.top || left.visual - right.visual)[0].page;
  });
  const pageNumber = page.locator('.page-number-field input');
  await expect(pageNumber).toHaveValue(String(expectedCurrent + 1));
  const firstMounted = Number(await page.locator('.virtual-page-row').first().getAttribute('data-source-page-index'));
  if (firstMounted !== expectedCurrent) {
    await expect(page.locator('.virtual-page-row').first()).toHaveAttribute('data-visible', 'false');
  }
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await expect.poll(async () => Number(await pageNumber.inputValue())).toBeGreaterThan(1);
  await page.getByRole('button', { name: 'Fit width' }).click();
  await expect.poll(() => page.locator('.virtual-page-row[data-visible="true"]').count()).toBeGreaterThan(0);
  expect(await page.locator('.pdf-page-surface').count()).toBeLessThanOrEqual(6);
});

test('restores Open PDF focus after user Close and empty password Cancel only', async ({ page }, testInfo) => {
  await installTransport(page, 3);
  await openViewer(page);
  await page.keyboard.press('Control+f');
  await page.getByPlaceholder('Find in document').focus();
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  const openButton = page.getByRole('button', { name: 'Open PDF', exact: true }).first();
  await expect(openButton).toBeFocused();
  await openButton.click();
  const thumbnail = page.getByRole('button', { name: 'Page 1 of 3' }).first();
  await thumbnail.focus();
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(openButton).toBeFocused();

  const clearPath = testInfo.outputPath('focus-clear.pdf');
  const encryptedPath = testInfo.outputPath('focus-encrypted.pdf');
  await writeFile(clearPath, syntheticPdf(1));
  execFileSync(
    resolve(import.meta.dirname, '..', 'src-tauri', 'resources', 'qpdf', '12.3.2', 'bin', 'qpdf.exe'),
    [clearPath, '--encrypt', 'focus-secret', 'test-owner', '256', '--', encryptedPath],
    { stdio: 'pipe' },
  );
  await page.goto('/');
  await installTransport(page, 1, await readFile(encryptedPath));
  await page.reload();
  await page.getByRole('button', { name: 'Viewer', exact: true }).click();
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().click();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByRole('button', { name: 'Open PDF', exact: true }).first()).toBeFocused();
  await page.goto('about:blank');
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('BODY');
});

type PerformanceSample = {
  acknowledgementMs: number;
  sessionMs: number;
  documentMs: number;
  firstPageMs: number;
  firstThumbnailMs: number;
  closeMs: number;
  heapBytes: number | null;
  heapGrowthBytes: number | null;
  retainedFullCanvases: number;
  retainedThumbnails: number;
};

type InteractionSample = {
  visiblePageMs: number;
  zoomMs: number;
  searchFirstResultMs: number;
  meanScrollFrameMs: number;
};

function summary(values: number[]) {
  const sorted = [...values].sort((left, right) => left - right);
  const percentile = (value: number) => sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * value) - 1)];
  return { median: percentile(0.5), p95: percentile(0.95) };
}

async function readChromiumHeap(page: Page): Promise<number | null> {
  const session = await page.context().newCDPSession(page);
  try {
    await session.send('Performance.enable');
    const result = await session.send('Performance.getMetrics');
    return result.metrics.find((metric) => metric.name === 'JSHeapUsedSize')?.value ?? null;
  } finally {
    await session.detach();
  }
}

async function measureOpenOnly(page: Page): Promise<Omit<PerformanceSample, 'closeMs'>> {
  const baselineHeapBytes = await readChromiumHeap(page);
  await page.getByRole('button', { name: 'Open PDF', exact: true }).first().evaluate((button) => {
    performance.clearMarks();
    performance.mark('g03-test-open-start');
    (button as HTMLButtonElement).click();
  });
  await expect.poll(() => page.locator('.pdf-page-surface canvas').first().evaluate((canvas) => (canvas as HTMLCanvasElement).width)).toBeGreaterThan(0);
  await expect.poll(() => page.locator('.page-thumbnail canvas').first().evaluate((canvas) => (canvas as HTMLCanvasElement).width)).toBeGreaterThan(0);
  const timings = await page.evaluate(() => {
    const start = performance.getEntriesByName('g03-test-open-start').at(-1)?.startTime ?? 0;
    const sinceStart = (name: string) => (performance.getEntriesByName(name).at(-1)?.startTime ?? start) - start;
    return {
      acknowledgementMs: sinceStart('g03-open-acknowledged'),
      sessionMs: sinceStart('g03-viewer-session-received'),
      documentMs: sinceStart('g03-pdf-document-ready'),
      firstPageMs: sinceStart('g03-first-page-displayed'),
      firstThumbnailMs: sinceStart('g03-first-thumbnail-displayed'),
    };
  });
  const heapBytes = await readChromiumHeap(page);
  const retained = await page.evaluate(() => ({
    retainedFullCanvases: document.querySelectorAll('.pdf-page-surface canvas').length,
    retainedThumbnails: document.querySelectorAll('.page-thumbnail canvas').length,
  }));
  return {
    ...timings,
    heapBytes,
    heapGrowthBytes: heapBytes === null || baselineHeapBytes === null
      ? null
      : Math.max(0, heapBytes - baselineHeapBytes),
    ...retained,
  };
}

async function closeViewer(page: Page): Promise<number> {
  const closeStarted = Date.now();
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Open PDF', exact: true }).first()).toBeEnabled();
  await expect(page.locator('.pdf-page-surface')).toHaveCount(0);
  return Date.now() - closeStarted;
}

async function measureInteractions(page: Page): Promise<InteractionSample> {
  await page.keyboard.press('Control+f');
  const searchStarted = Date.now();
  await page.getByPlaceholder('Find in document').fill('Page 2 unique-text-2');
  await expect(page.getByText(/1 of 1/)).toBeVisible();
  const searchFirstResultMs = Date.now() - searchStarted;

  const canvas = page.locator('.pdf-page-surface canvas').first();
  const priorWidth = await canvas.evaluate((element) => (element as HTMLCanvasElement).width);
  const zoomStarted = Date.now();
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.waitForFunction(
    (width) => document.querySelector<HTMLCanvasElement>('.pdf-page-surface canvas')?.width !== width,
    priorWidth,
    { polling: 'raf' },
  );
  const zoomMs = Date.now() - zoomStarted;

  const meanScrollFrameMs = await page.locator('.page-canvas-scroll').evaluate(async (element) => {
    const started = performance.now();
    for (let step = 0; step < 20; step += 1) {
      element.scrollTop += 300;
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }
    return (performance.now() - started) / 20;
  });

  const visiblePageStarted = Date.now();
  await page.getByRole('button', { name: 'Last page' }).click();
  await expect.poll(() => page.locator('[data-page-index="999"] canvas').evaluate((element) => (element as HTMLCanvasElement).width)).toBeGreaterThan(0);
  const visiblePageMs = Date.now() - visiblePageStarted;
  return { visiblePageMs, zoomMs, searchFirstResultMs, meanScrollFrameMs };
}

for (const pageCount of [100, 1000]) {
  test(`records cold and warm progressive-viewer measurements for ${pageCount} pages`, async ({ page }) => {
    test.slow();
    await installTransport(page, pageCount);
    const cold: PerformanceSample[] = [];
    const warm: PerformanceSample[] = [];
    const interactions: InteractionSample[] = [];
    for (let run = 0; run < 5; run += 1) {
      await page.goto('/');
      await page.getByRole('button', { name: 'Viewer', exact: true }).click();
      const coldOpen = await measureOpenOnly(page);
      cold.push({ ...coldOpen, closeMs: await closeViewer(page) });
      const warmOpen = await measureOpenOnly(page);
      if (pageCount === 1000) interactions.push(await measureInteractions(page));
      warm.push({ ...warmOpen, closeMs: await closeViewer(page) });
    }
    const evidence = await page.evaluate(() => window.__G03_TEST_EVIDENCE__);
    const renderCounts = await page.evaluate(() => ({
      fullCanvases: document.querySelectorAll('.pdf-page-surface canvas').length,
      thumbnails: document.querySelectorAll('.page-thumbnail canvas').length,
    }));
    const summarize = (samples: PerformanceSample[]) => ({
      acknowledgementMs: summary(samples.map((sample) => sample.acknowledgementMs)),
      sessionMs: summary(samples.map((sample) => sample.sessionMs)),
      documentMs: summary(samples.map((sample) => sample.documentMs)),
      firstPageMs: summary(samples.map((sample) => sample.firstPageMs)),
      firstThumbnailMs: summary(samples.map((sample) => sample.firstThumbnailMs)),
      closeMs: summary(samples.map((sample) => sample.closeMs)),
      peakHeapBytes: Math.max(...samples.map((sample) => sample.heapBytes ?? 0)),
      peakHeapGrowthBytes: Math.max(...samples.map((sample) => sample.heapGrowthBytes ?? 0)),
      maximumRetainedFullCanvases: Math.max(...samples.map((sample) => sample.retainedFullCanvases)),
      maximumRetainedThumbnails: Math.max(...samples.map((sample) => sample.retainedThumbnails)),
    });
    console.log(`G03_PERF_${pageCount}=${JSON.stringify({
      pageCount,
      fixtureBytes: evidence.fixtureBytes,
      samplesPerTemperature: 5,
      cold: summarize(cold),
      warm: summarize(warm),
      interactions: interactions.length === 0 ? null : {
        visiblePageMs: summary(interactions.map((sample) => sample.visiblePageMs)),
        zoomMs: summary(interactions.map((sample) => sample.zoomMs)),
        searchFirstResultMs: summary(interactions.map((sample) => sample.searchFirstResultMs)),
        meanScrollFrameMs: summary(interactions.map((sample) => sample.meanScrollFrameMs)),
      },
      peakReads: evidence.peakReads,
      renderedAfterClose: renderCounts,
    })}`);
    expect(evidence.peakReads).toBeLessThanOrEqual(4);
    expect(renderCounts).toEqual({ fullCanvases: 0, thumbnails: 0 });
  });
}
