import { defineConfig } from '@playwright/test';
import { resolve } from 'node:path';

process.env.PLAYWRIGHT_BROWSERS_PATH ??= resolve(import.meta.dirname, '..', '..', '.cache', 'ms-playwright');
const browserEvidenceRoot = process.env.DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT
  ? resolve(process.env.DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT)
  : null;

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  outputDir: browserEvidenceRoot
    ? resolve(browserEvidenceRoot, 'playwright-test-results')
    : 'test-results',
  reporter: [['list'], ['json', {
    outputFile: browserEvidenceRoot
      ? resolve(browserEvidenceRoot, 'g03-browser-results.json')
      : 'test-results/g03-browser-results.json',
  }]],
  webServer: {
    command: 'npm run dev:test-browser -- --host 127.0.0.1',
    url: 'http://127.0.0.1:1420',
    reuseExistingServer: false,
    timeout: 60_000,
  },
  use: {
    baseURL: 'http://127.0.0.1:1420',
    browserName: 'chromium',
    headless: true,
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
});
