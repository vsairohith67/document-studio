import { describe, expect, it } from 'vitest';
import {
  calculateSizeReport,
  formatSignedBytes,
  formatSignedPercentage,
  sizeOutcomeLabel,
} from './sizeReporting';

describe('truthful deterministic size reporting', () => {
  it.each([
    [1000, 750, 'smaller', -250, -25, 'Saved 250 B (25.00%)'],
    [1000, 1000, 'no-meaningful-change', 0, 0, 'No size change (0 B, 0.00%)'],
    [1000, 1250, 'larger', 250, 25, 'Output grew by 250 B (+25.00%)'],
  ] as const)('reports %i to %i truthfully', (before, after, outcome, delta, percent, label) => {
    const report = calculateSizeReport(before, after);
    expect(report).toMatchObject({ outcome, deltaBytes: delta, percentageDelta: percent });
    expect(sizeOutcomeLabel(report)).toBe(label);
  });

  it('does not turn a tiny increase into a positive saved percentage', () => {
    const report = calculateSizeReport(20_000, 20_001);
    expect(report.outcome).toBe('no-meaningful-change');
    expect(sizeOutcomeLabel(report)).toBe('No meaningful size change (+1 B, +0.01%)');
    expect(sizeOutcomeLabel(report)).not.toContain('Saved');
  });

  it('uses stable signed formatting', () => {
    expect(formatSignedBytes(-1024)).toBe('−1.0 KB');
    expect(formatSignedBytes(12)).toBe('+12 B');
    expect(formatSignedPercentage(-1 / 3)).toBe('−0.33%');
  });
});
