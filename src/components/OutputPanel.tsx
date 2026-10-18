import { useEffect, useRef } from "react";
import type { RunOutput, RunResult } from "../services/runtime";

interface Props {
  lines: RunOutput[];
  running: boolean;
  result: RunResult | null;
  height: number;
  onClear: () => void;
}

function statusChip(running: boolean, r: RunResult | null) {
  if (running) return <span className="chip pending">Running…</span>;
  if (!r) return <span className="chip idle">Idle</span>;
  if (r.error) return <span className="chip bad">Could not run</span>;
  if (r.killed) return <span className="chip bad">Stopped</span>;
  return r.exitCode === 0 ? (
    <span className="chip ok">Exit status 0</span>
  ) : (
    <span className="chip bad">Exit status {r.exitCode ?? "unknown"}</span>
  );
}

export default function OutputPanel({ lines, running, result, height, onClear }: Props) {
  const endRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [lines.length, result]);

  return (
    <section className="output" style={{ height }}>
      <header>
        <span className="title">Output</span>
        {statusChip(running, result)}
        {result && !running && <span className="muted">{(result.durationMs / 1000).toFixed(2)} s</span>}
        <div className="spacer" />
        <button onClick={onClear} disabled={running || (lines.length === 0 && !result)}>Clear</button>
      </header>
      <div className="lines mono">
        {result?.error && <div className="line stderr">{result.error}</div>}
        {lines.map((l, i) => (
          <div key={i} className={`line ${l.stream}`}>{l.text || " "}</div>
        ))}
        {!running && !result && lines.length === 0 && (
          <div className="muted">Run a saved .py file to see stdout, stderr and the exit status here.</div>
        )}
        <div ref={endRef} />
      </div>
    </section>
  );
}
