import type { RuntimeStatus } from "../services/runtime";

interface Props {
  status: RuntimeStatus | null;
  loading: boolean;
  selectError: string | null;
  onRedetect: () => void;
  onSelect: () => void;
}

export default function RuntimePanel({ status, loading, selectError, onRedetect, onSelect }: Props) {
  const verified = status?.verified === true;
  return (
    <section className="runtime">
      <h2>Python runtime</h2>

      {loading && <div className="pill pending">Detecting…</div>}
      {!loading && status && (
        <div className={verified ? "pill ok" : "pill bad"}>
          {verified ? "Verified — Ignytion managed" : "Not verified — Run disabled"}
        </div>
      )}

      {verified && status && (
        <dl>
          <dt>Interpreter</dt>
          <dd className="mono">{status.pythonPath}</dd>
          <dt>Version</dt>
          <dd>{status.version}</dd>
          <dt>Package dir</dt>
          <dd className="mono">{status.packageDir}</dd>
          <dt>Found via</dt>
          <dd>{status.source}</dd>
        </dl>
      )}

      {!loading && status && !verified && <pre className="hint">{status.message}</pre>}
      {selectError && <pre className="hint rejected">{selectError}</pre>}

      <div className="row">
        <button onClick={onRedetect} disabled={loading}>Re-detect</button>
        <button onClick={onSelect} disabled={loading}>Select interpreter…</button>
      </div>

      {status && status.checks.length > 0 && (
        <details>
          <summary>How this was verified</summary>
          <ul className="checks">
            {status.checks.map((c, i) => (
              <li key={i} className={c.ok ? "ok" : "bad"}>
                <span className="mark">{c.ok ? "✓" : "✗"}</span>
                <span>
                  <strong>{c.name}</strong>
                  <span className="mono detail">{c.detail}</span>
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
