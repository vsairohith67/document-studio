export type SizeOutcome = 'smaller' | 'no-meaningful-change' | 'larger';

export interface SizeReport {
  beforeBytes: number;
  afterBytes: number;
  deltaBytes: number;
  percentageDelta: number;
  outcome: SizeOutcome;
}

export function calculateSizeReport(beforeBytes: number, afterBytes: number): SizeReport {
  const deltaBytes = afterBytes - beforeBytes;
  const percentageDelta = beforeBytes === 0 ? 0 : (deltaBytes / beforeBytes) * 100;
  const outcome: SizeOutcome = deltaBytes > 0 && Math.abs(percentageDelta) >= 0.01
    ? 'larger'
    : deltaBytes < 0 && Math.abs(percentageDelta) >= 0.01
      ? 'smaller'
      : 'no-meaningful-change';
  return { beforeBytes, afterBytes, deltaBytes, percentageDelta, outcome };
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatSignedBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  return `${bytes > 0 ? '+' : '−'}${formatBytes(Math.abs(bytes))}`;
}

export function formatSignedPercentage(value: number): string {
  if (value === 0) return '0.00%';
  return `${value > 0 ? '+' : '−'}${Math.abs(value).toFixed(2)}%`;
}

export function sizeOutcomeLabel(report: SizeReport): string {
  if (report.outcome === 'smaller') {
    return `Saved ${formatBytes(Math.abs(report.deltaBytes))} (${Math.abs(report.percentageDelta).toFixed(2)}%)`;
  }
  if (report.outcome === 'larger') {
    return `Output grew by ${formatBytes(report.deltaBytes)} (+${report.percentageDelta.toFixed(2)}%)`;
  }
  return report.deltaBytes === 0
    ? 'No size change (0 B, 0.00%)'
    : `No meaningful size change (${formatSignedBytes(report.deltaBytes)}, ${formatSignedPercentage(report.percentageDelta)})`;
}
