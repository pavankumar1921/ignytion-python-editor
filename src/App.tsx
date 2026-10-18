import { useCallback, useEffect, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/editor/editor.worker?worker";
import Editor, { loader, type OnMount } from "@monaco-editor/react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import RuntimePanel from "./components/RuntimePanel";
import OutputPanel from "./components/OutputPanel";
import ChatPanel, { type SelectionInfo } from "./components/ChatPanel";
import {
  detectRuntime,
  onRunOutput,
  runPython,
  selectInterpreter,
  stopPython,
  type RunOutput,
  type RunResult,
  type RuntimeStatus,
} from "./services/runtime";
import "./App.css";

// Bundle Monaco locally so the editor works offline (no CDN fetch).
self.MonacoEnvironment = { getWorker: () => new editorWorker() };
loader.config({ monaco });

const STARTER = `# New Ignytion script
name = "Ignytion"
print(f"Hello from {name}!")
print(10 + 20)
`;

const PY_FILTER = [{ name: "Python", extensions: ["py"] }];
const MAX_UI_LINES = 5000;

function fileName(path: string | null): string {
  if (!path) return "untitled.py";
  return path.split(/[\\/]/).pop() ?? path;
}

export default function App() {
  // ---- document state
  const [path, setPath] = useState<string | null>(null);
  const [content, setContent] = useState(STARTER);
  const [savedContent, setSavedContent] = useState(STARTER);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const dirty = content !== savedContent;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const contentRef = useRef(content);
  contentRef.current = content;

  // ---- editor handle + selection (used by the AI chat, only when the user opts in)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const [selection, setSelection] = useState<SelectionInfo | null>(null);
  const readSelection = useCallback((): { text: string; info: SelectionInfo } | null => {
    const ed = editorRef.current;
    const sel = ed?.getSelection();
    const model = ed?.getModel();
    if (!ed || !sel || !model || sel.isEmpty()) return null;
    const text = model.getValueInRange(sel);
    return { text, info: { startLine: sel.startLineNumber, endLine: sel.endLineNumber, chars: text.length } };
  }, []);
  const onEditorMount: OnMount = useCallback(
    (editor) => {
      editorRef.current = editor;
      editor.onDidChangeCursorSelection(() => setSelection(readSelection()?.info ?? null));
    },
    [readSelection],
  );

  // ---- runtime + run state
  const [runtime, setRuntime] = useState<RuntimeStatus | null>(null);
  const [runtimeLoading, setRuntimeLoading] = useState(true);
  const [selectError, setSelectError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [lines, setLines] = useState<RunOutput[]>([]);
  const [result, setResult] = useState<RunResult | null>(null);
  const [outputHeight, setOutputHeight] = useState(220);

  // ---- output streaming (batched per animation frame so a chatty script can't freeze the UI)
  const bufferRef = useRef<RunOutput[]>([]);
  const rafRef = useRef<number | null>(null);
  const flush = useCallback(() => {
    rafRef.current = null;
    const batch = bufferRef.current;
    bufferRef.current = [];
    if (batch.length === 0) return;
    setLines((prev) => {
      const next = prev.concat(batch);
      return next.length > MAX_UI_LINES ? next.slice(next.length - MAX_UI_LINES) : next;
    });
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onRunOutput((o) => {
      bufferRef.current.push(o);
      if (rafRef.current === null) rafRef.current = requestAnimationFrame(flush);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [flush]);

  // ---- runtime detection
  const redetect = useCallback(async () => {
    setRuntimeLoading(true);
    setSelectError(null);
    try {
      setRuntime(await detectRuntime());
    } catch (e) {
      setRuntime({
        verified: false,
        pythonPath: null,
        version: null,
        packageDir: null,
        source: "marker",
        message: `Runtime detection failed: ${String(e)}`,
        checks: [],
      });
    } finally {
      setRuntimeLoading(false);
    }
  }, []);

  useEffect(() => {
    void redetect();
  }, [redetect]);

  const chooseInterpreter = useCallback(async () => {
    setSelectError(null);
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        title: "Select the Ignytion-managed Python interpreter",
      });
      if (typeof picked !== "string") return;
      const st = await selectInterpreter(picked);
      if (st.verified) {
        setRuntime(st);
      } else {
        const why = st.checks.filter((c) => !c.ok).map((c) => `• ${c.name}: ${c.detail}`);
        setSelectError(`Rejected: ${picked}\n${st.message.split("\n\n")[0]}\n${why.join("\n")}`);
      }
    } catch (e) {
      setSelectError(String(e));
    }
  }, []);

  // ---- file operations
  const confirmDiscard = useCallback(async () => {
    if (!dirtyRef.current) return true;
    return ask("You have unsaved changes. Discard them?", {
      title: "Unsaved changes",
      kind: "warning",
      okLabel: "Discard",
      cancelLabel: "Cancel",
    });
  }, []);

  const resetOutput = useCallback(() => {
    bufferRef.current = [];
    setLines([]);
    setResult(null);
  }, []);

  const newFile = useCallback(async () => {
    if (running || !(await confirmDiscard())) return;
    setPath(null);
    setContent(STARTER);
    setSavedContent(STARTER);
    setError(null);
    setNotice(null);
    resetOutput();
  }, [confirmDiscard, resetOutput, running]);

  const openFile = useCallback(async () => {
    if (running || !(await confirmDiscard())) return;
    try {
      const selected = await openDialog({ multiple: false, directory: false, filters: PY_FILTER });
      if (typeof selected !== "string") return;
      const text = await invoke<string>("read_text_file", { path: selected });
      setPath(selected);
      setContent(text);
      setSavedContent(text);
      setError(null);
      setNotice(`Opened ${selected}`);
      resetOutput();
    } catch (e) {
      setError(String(e));
    }
  }, [confirmDiscard, resetOutput, running]);

  /** Returns the saved path, or null if cancelled/failed. */
  const saveFile = useCallback(
    async (forceDialog: boolean): Promise<string | null> => {
      try {
        let target = path;
        if (!target || forceDialog) {
          const chosen = await saveDialog({ defaultPath: target ?? "untitled.py", filters: PY_FILTER });
          if (!chosen) return null;
          target = chosen.toLowerCase().endsWith(".py") ? chosen : `${chosen}.py`;
        }
        await invoke("write_text_file", { path: target, contents: content });
        setPath(target);
        setSavedContent(content);
        setError(null);
        setNotice(`Saved ${target}`);
        return target;
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
    [path, content],
  );

  // ---- run / stop
  const canRun = runtime?.verified === true && !runtimeLoading && !running;
  const runDisabledReason = runtimeLoading
    ? "Detecting the Ignytion Python runtime…"
    : runtime?.verified !== true
      ? "Run is disabled: no verified Ignytion-managed Python. See the Python runtime panel."
      : running
        ? "A script is already running"
        : "Run the current file with the Ignytion-managed Python (saves first)";

  const run = useCallback(async () => {
    if (!canRun) return;
    // Never run an unsaved buffer as though it were the saved file: save first.
    let target = path;
    if (!target || dirtyRef.current) {
      target = await saveFile(false);
      if (!target) return; // cancelled or failed; error banner already set
    }
    resetOutput();
    setRunning(true);
    try {
      const res = await runPython(target);
      flush();
      setResult(res);
    } catch (e) {
      setResult({ exitCode: null, killed: false, durationMs: 0, error: String(e) });
    } finally {
      setRunning(false);
    }
  }, [canRun, flush, path, resetOutput, saveFile]);

  const stop = useCallback(async () => {
    try {
      await stopPython();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // ---- keyboard: Ctrl/Cmd+S saves, Ctrl/Cmd+Shift+S = Save As, F5 runs
  useEffect(() => {
    const onKey = (ev: KeyboardEvent) => {
      if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "s") {
        ev.preventDefault();
        void saveFile(ev.shiftKey);
      } else if (ev.key === "F5") {
        ev.preventDefault();
        void run();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [run, saveFile]);

  // ---- warn before closing with unsaved work
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      if (!dirtyRef.current) return;
      const discard = await ask("You have unsaved changes. Close without saving?", {
        title: "Unsaved changes",
        kind: "warning",
        okLabel: "Close anyway",
        cancelLabel: "Stay",
      });
      if (!discard) event.preventDefault();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // ---- output panel splitter
  const dragRef = useRef<{ startY: number; startH: number } | null>(null);
  const onSplitDown = (ev: React.PointerEvent<HTMLDivElement>) => {
    dragRef.current = { startY: ev.clientY, startH: outputHeight };
    ev.currentTarget.setPointerCapture(ev.pointerId);
  };
  const onSplitMove = (ev: React.PointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    const delta = dragRef.current.startY - ev.clientY;
    setOutputHeight(Math.min(600, Math.max(80, dragRef.current.startH + delta)));
  };
  const onSplitUp = () => {
    dragRef.current = null;
  };

  return (
    <div className="app">
      <header className="toolbar">
        <div className="brand">
          IGNYTION <span className="amber">io</span>
        </div>
        <button onClick={() => void newFile()} disabled={running}>New</button>
        <button onClick={() => void openFile()} disabled={running}>Open</button>
        <button onClick={() => void saveFile(false)} disabled={!dirty && path !== null}>Save</button>
        <button onClick={() => void saveFile(true)}>Save As</button>
        <div className="spacer" />
        {running ? (
          <button className="stop" onClick={() => void stop()}>■ Stop</button>
        ) : (
          <button className="run" onClick={() => void run()} disabled={!canRun} title={runDisabledReason}>
            ▶ Run
          </button>
        )}
      </header>

      <div className="body">
        <aside className="sidebar">
          <RuntimePanel
            status={runtime}
            loading={runtimeLoading}
            selectError={selectError}
            onRedetect={() => void redetect()}
            onSelect={() => void chooseInterpreter()}
          />
        </aside>

        <div className="main">
          <div className="tab">
            <span className={dirty ? "dot dirty" : "dot"} title={dirty ? "Unsaved changes" : "Saved"} />
            <span className="tabname">{fileName(path)}</span>
            {dirty && <span className="unsaved">unsaved changes</span>}
            {path && <span className="fullpath">{path}</span>}
          </div>

          {error && <div className="banner error">{error}</div>}

          <div className="editor">
            <Editor
              height="100%"
              language="python"
              theme="vs-dark"
              value={content}
              onMount={onEditorMount}
              onChange={(v) => setContent(v ?? "")}
              options={{ lineNumbers: "on", minimap: { enabled: false }, fontSize: 14, automaticLayout: true }}
            />
          </div>

          <div
            className="splitter"
            onPointerDown={onSplitDown}
            onPointerMove={onSplitMove}
            onPointerUp={onSplitUp}
            title="Drag to resize the output panel"
          />
          <OutputPanel lines={lines} running={running} result={result} height={outputHeight} onClear={resetOutput} />
        </div>

        <aside className="chat">
          <ChatPanel
            fileName={fileName(path)}
            fileLines={content.split("\n").length}
            selection={selection}
            getSelectionText={() => readSelection()?.text ?? ""}
            getFileText={() => contentRef.current}
          />
        </aside>
      </div>

      <footer className="status">{notice ?? "Ready"}</footer>
    </div>
  );
}
