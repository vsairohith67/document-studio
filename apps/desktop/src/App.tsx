import { useEffect, useMemo, useState } from 'react';
import type {
  DependencyDiagnostic,
  FileInspection,
  JobRecord,
  ProgressEvent,
  SystemStatus,
} from '@document-studio/contracts';
import { api, createProgressReconciler, operationErrorMessage } from './api';

const terminalStates = new Set(['completed', 'failed', 'cancelled']);

function copyOutputName(displayName: string): string {
  const dot = displayName.lastIndexOf('.');
  if (dot > 0) {
    return `${displayName.slice(0, dot)}-copy${displayName.slice(dot)}`;
  }
  return `${displayName}-copy`;
}

function shortPath(path: string | null | undefined): string {
  if (!path) return 'Not available';
  return path.replaceAll('\\', '/').split('/').filter(Boolean).at(-1) ?? path;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function App() {
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [input, setInput] = useState<FileInspection | null>(null);
  const [destination, setDestination] = useState<string | null>(null);
  const [outputName, setOutputName] = useState('');
  const [job, setJob] = useState<JobRecord | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [history, setHistory] = useState<JobRecord[]>([]);
  const [dependencies, setDependencies] = useState<DependencyDiagnostic[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshHistory = async () => {
    setHistory(await api.history.list({ limit: 8 }));
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const reconciler = createProgressReconciler(
      (jobId) => api.jobs.get({ jobId }),
      (snapshot) => active && setJob(snapshot),
      (event) => {
        if (!active) return;
        setProgress(event);
        if (terminalStates.has(event.state)) {
          void api.jobs.get({ jobId: event.jobId }).then((snapshot) => {
            if (active) setJob(snapshot);
          });
          void refreshHistory();
          setBusy(false);
        }
      },
    );

    void Promise.all([
      api.system.status(),
      api.dependencies.scan(),
      api.history.list({ limit: 8 }),
    ])
      .then(([status, dependencyStatus, jobHistory]) => {
        if (!active) return;
        setSystem(status);
        setDependencies(dependencyStatus);
        setHistory(jobHistory);
      })
      .catch((reason: unknown) => active && setError(operationErrorMessage(reason)));
    void api.jobs.onProgress((event) => {
      void reconciler(event).catch((reason: unknown) => {
        if (active) setError(operationErrorMessage(reason));
      });
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const chooseInput = async () => {
    setError(null);
    const path = await api.dialogs.selectInput();
    if (!path) return;
    try {
      const [inspection] = await api.files.inspect([path]);
      if (!inspection) throw new Error('No file was selected');
      setInput(inspection);
      setOutputName(copyOutputName(inspection.displayName));
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const chooseDestination = async () => {
    setError(null);
    const path = await api.dialogs.selectDestination();
    if (path) setDestination(path);
  };

  const startCopy = async () => {
    if (!input || !destination || !outputName) return;
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      const created = await api.jobs.create({
        operationId: 'diagnostic.copy',
        inputPaths: [input.path],
        destinationDirectory: destination,
        requestedOutputName: outputName,
      });
      setJob(created);
    } catch (reason) {
      setBusy(false);
      setError(operationErrorMessage(reason));
    }
  };

  const cancelCopy = async () => {
    if (!job) return;
    try {
      await api.jobs.cancel({ jobId: job.id });
    } catch (reason) {
      setError(operationErrorMessage(reason));
    }
  };

  const progressValue = progress?.completedUnits ?? job?.progress.completedUnits ?? 0;
  const progressTotal = progress?.totalUnits ?? job?.progress.totalUnits ?? 0;
  const progressPercent = progressTotal === 0
    ? job?.state === 'completed' ? 100 : 0
    : Math.min(100, Math.round((progressValue / progressTotal) * 100));
  const cancellable = progress?.cancellable ?? Boolean(job && !terminalStates.has(job.state));
  const coreDependencies = useMemo(
    () => dependencies.filter((dependency) => dependency.status === 'available'),
    [dependencies],
  );
  const deferredDependencies = useMemo(
    () => dependencies.filter((dependency) => dependency.status === 'not-required'),
    [dependencies],
  );

  return (
    <div className="app-shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand" aria-label="Document Studio">DS</div>
        <button className="rail-button active" aria-current="page">Home</button>
        <button className="rail-button" disabled>Viewer</button>
        <button className="rail-button" disabled>Tools</button>
        <button className="rail-button" disabled>Settings</button>
      </aside>

      <main className="workspace">
        <header className="page-header">
          <div>
            <p className="eyebrow">WINDOWS FOUNDATION</p>
            <h1>Document Studio is ready for local diagnostics</h1>
            <p className="lede">Prove secure processing with a verified copy before document engines are added.</p>
          </div>
          <div className="privacy-badge">
            <span aria-hidden="true">●</span>
            {system?.offlineByDefault ? 'Offline by default' : 'Checking local status'}
          </div>
        </header>

        {error && <div className="error-banner" role="alert">{error}</div>}

        <section className="foundation-grid" aria-label="Foundation tools">
          <article className="card copy-card">
            <div className="card-heading">
              <div>
                <p className="eyebrow">REFERENCE OPERATION</p>
                <h2>Diagnostic copy</h2>
              </div>
              <span className="status-chip">SHA-256 verified</span>
            </div>
            <p>Select one local file and a local destination. Existing files are never replaced.</p>

            <div className="selection-row">
              <div>
                <span className="field-label">Input file</span>
                <strong>{input?.displayName ?? 'No file selected'}</strong>
                <small>{input ? formatBytes(input.sizeBytes) : 'The source remains in its current location.'}</small>
              </div>
              <button type="button" className="secondary" onClick={chooseInput} disabled={busy}>Choose file</button>
            </div>

            <div className="selection-row">
              <div>
                <span className="field-label">Destination</span>
                <strong>{destination ? 'Local folder selected' : 'No folder selected'}</strong>
                <small>A verified output is published here with collision-safe naming.</small>
              </div>
              <button type="button" className="secondary" onClick={chooseDestination} disabled={busy}>Choose folder</button>
            </div>

            <label className="output-field">
              <span className="field-label">Output name</span>
              <input
                value={outputName}
                onChange={(event) => setOutputName(event.target.value)}
                placeholder="example-copy.pdf"
                disabled={busy}
              />
            </label>

            <div className="action-row">
              <button
                type="button"
                className="primary"
                disabled={!input || !destination || !outputName || busy}
                onClick={startCopy}
              >
                Run verified copy
              </button>
              {busy && (
                <button type="button" className="secondary danger" disabled={!cancellable} onClick={cancelCopy}>
                  {cancellable ? 'Cancel' : 'Publishing safely'}
                </button>
              )}
            </div>

            <div className="job-status" aria-live="polite" aria-atomic="true">
              <div className="job-status-line">
                <strong>{progress?.message ?? (job ? `Job ${job.state}` : 'No active job')}</strong>
                <span>{progressPercent}%</span>
              </div>
              <progress value={progressPercent} max={100} aria-label="Diagnostic copy progress" />
              {job?.state === 'completed' && (
                <p className="success-copy">Verified output: {shortPath(job.outputs[0]?.finalPath)}</p>
              )}
            </div>
          </article>

          <aside className="side-stack">
            <article className="card diagnostics-card">
              <div className="card-heading">
                <h2>Dependency diagnostics</h2>
                <span className="status-dot" aria-hidden="true" />
              </div>
              <ul className="diagnostic-list">
                {coreDependencies.map((dependency) => (
                  <li key={dependency.id}>
                    <span><strong>{dependency.id}</strong><small>{dependency.version ?? 'Built in'}</small></span>
                    <span className="available">Available</span>
                  </li>
                ))}
              </ul>
              <p className="deferred-note">
                {deferredDependencies.length} future engines are not required and were not installed.
              </p>
            </article>

            <article className="card placeholder-card" aria-labelledby="viewer-heading">
              <p className="eyebrow">FUTURE GOAL</p>
              <h2 id="viewer-heading">Document viewer</h2>
              <div className="paper-placeholder" aria-hidden="true">
                <span />
                <span />
                <span />
              </div>
              <p>PDF rendering and virtualization arrive in G03. No PDF runtime is loaded in this foundation.</p>
              <button type="button" className="secondary" disabled>Viewer unavailable in G01</button>
            </article>
          </aside>
        </section>

        <section className="history-section" aria-labelledby="history-heading">
          <div className="section-heading">
            <div>
              <p className="eyebrow">METADATA ONLY</p>
              <h2 id="history-heading">Recent jobs</h2>
            </div>
            <span>30-day default history</span>
          </div>
          <div className="history-list">
            {history.length === 0 && <p className="empty-state">Completed, failed, and cancelled jobs will appear here.</p>}
            {history.map((item) => (
              <article className="history-row" key={item.id}>
                <div>
                  <strong>{shortPath(item.inputs[0]?.displayName)}</strong>
                  <small>{item.operationId} · {new Date(item.updatedAt).toLocaleString()}</small>
                </div>
                <span className={`job-state state-${item.state}`}>{item.state}</span>
                <span>{item.outputs[0]?.sizeBytes == null ? '—' : formatBytes(item.outputs[0].sizeBytes)}</span>
              </article>
            ))}
          </div>
        </section>
      </main>
    </div>
  );
}
