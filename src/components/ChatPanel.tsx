import { useCallback, useEffect, useRef, useState } from "react";
import {
  clearKey,
  hostOf,
  keyStatus,
  loadConfig,
  presetFor,
  PRESETS,
  saveConfig,
  saveKey,
  sendChat,
  toAiError,
  type AiConfig,
  type AiError,
  type ChatMessage,
  type CodeContext,
} from "../services/ai"

export interface SelectionInfo {
  startLine: number;
  endLine: number;
  chars: number;
}

interface Props {
  fileName: string;
  fileLines: number;
  selection: SelectionInfo | null;
  getSelectionText: () => string;
  getFileText: () => string;
}

type CodeChoice = "none" | "selection" | "file";

interface Msg {
  id: number;
  role: "user" | "assistant" | "error";
  content: string;
  /** shown under a user message so it is always clear what left the machine */
  sentLabel?: string;
  error?: AiError;
  retry?: () => void;
  retryLabel?: string;
}

const ERROR_TITLES: Record<string, string> = {
  missing_key: "No API key",
  unauthorized: "API key rejected",
  rate_limited: "Rate limited",
  network: "Network problem",
  timeout: "Timed out",
  provider: "Provider error",
  bad_response: "Unreadable response",
  invalid_request: "Cannot send",
  consent_required: "Code not sent",
  keystore: "Credential store problem",
  unknown: "Something went wrong",
};

function RichText({ text }: { text: string }) {
  const parts: { code: boolean; lang: string; body: string }[] = [];
  const re = /```([\w+-]*)\n([\s\S]*?)```/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) parts.push({ code: false, lang: "", body: text.slice(last, m.index) });
    parts.push({ code: true, lang: m[1], body: m[2].replace(/\n$/, "") });
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push({ code: false, lang: "", body: text.slice(last) });

  return (
    <>
      {parts.map((p, i) =>
        p.code ? (
          <div className="codeblock" key={i}>
            <div className="codehead">
              <span>{p.lang || "code"}</span>
              <button onClick={() => void navigator.clipboard?.writeText(p.body)}>Copy</button>
            </div>
            <pre className="mono">{p.body}</pre>
          </div>
        ) : (
          <p key={i}>{p.body.trim()}</p>
        ),
      )}
    </>
  );
}

export default function ChatPanel({ fileName, fileLines, selection, getSelectionText, getFileText }: Props) {
  const [config, setConfig] = useState<AiConfig>(loadConfig);
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [settingsMsg, setSettingsMsg] = useState<string | null>(null);

  const [messages, setMessages] = useState<Msg[]>([]);
  const [history, setHistory] = useState<ChatMessage[]>([]);
  const [prompt, setPrompt] = useState("");
  const [choice, setChoice] = useState<CodeChoice>("none");
  const [loading, setLoading] = useState(false);
  const idRef = useRef(1);
  const endRef = useRef<HTMLDivElement>(null);

  const host = hostOf(config.baseUrl);
  const preset = presetFor(config.baseUrl);

  // A selection that disappears must not stay "chosen".
  useEffect(() => {
    if (choice === "selection" && !selection) setChoice("none");
  }, [choice, selection]);

  useEffect(() => {
    saveConfig(config);
    let alive = true;
    setHasKey(null);
    keyStatus(config.baseUrl)
      .then((v) => alive && setHasKey(v))
      .catch(() => alive && setHasKey(null));
    return () => {
      alive = false;
    };
  }, [config]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length, loading]);

  const push = (m: Omit<Msg, "id">) => setMessages((prev) => [...prev, { ...m, id: idRef.current++ }]);

  const doSend = useCallback(
    async (text: string, ctx: CodeContext | null, hist: ChatMessage[], showUser: boolean) => {
      if (showUser) {
        push({
          role: "user",
          content: text,
          sentLabel: ctx ? `Sent with your code: ${ctx.label} (${ctx.text.length} chars) → ${host}` : undefined,
        });
      }
      setLoading(true);
      try {
        const reply = await sendChat(config, hist, text, ctx, ctx !== null);
        push({ role: "assistant", content: reply.content });
        setHistory([...hist, { role: "user", content: text }, { role: "assistant", content: reply.content }]);
      } catch (e) {
        const err = toAiError(e);
        push({
          role: "error",
          content: err.message,
          error: err,
          retry: () => void doSend(text, ctx, hist, false),
          retryLabel: ctx ? "Retry (sends the same code again)" : "Retry",
        });
      } finally {
        setLoading(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [config, host],
  );

  const send = () => {
    const text = prompt.trim();
    if (!text || loading) return;
    if (!config.model.trim()) {
      setShowSettings(true);
      setSettingsMsg("Choose a model name first (see your provider's model list).");
      return;
    }
    let ctx: CodeContext | null = null;
    if (choice === "selection" && selection) {
      ctx = { kind: "selection", label: `${fileName}, lines ${selection.startLine}-${selection.endLine}`, text: getSelectionText() };
    } else if (choice === "file") {
      ctx = { kind: "file", label: `${fileName}, whole file`, text: getFileText() };
    }
    setPrompt("");
    setChoice("none"); // every code send needs a fresh, deliberate choice
    void doSend(text, ctx, history, true);
  };

  const onSaveKey = async () => {
    setSettingsMsg(null);
    try {
      await saveKey(config.baseUrl, keyInput);
      setKeyInput("");
      setHasKey(true);
      setSettingsMsg(`Key saved in your OS credential store for ${host}.`);
    } catch (e) {
      setSettingsMsg(String(e));
    }
  };
  const onClearKey = async () => {
    setSettingsMsg(null);
    try {
      await clearKey(config.baseUrl);
      setHasKey(false);
      setSettingsMsg(`Key for ${host} removed.`);
    } catch (e) {
      setSettingsMsg(String(e));
    }
  };

  const sendLabel = choice === "none" ? "Send" : "Send with code";
  const codeChars = choice === "selection" ? selection?.chars : choice === "file" ? getFileText().length : 0;

  return (
    <section className="chatpanel">
      <header className="chathead">
        <h2>AI chat</h2>
        <span className="muted host">{host}</span>
        <div className="spacer" />
        <button onClick={() => setShowSettings((s) => !s)}>{showSettings ? "Close" : "Settings"}</button>
      </header>

      {showSettings && (
        <div className="settings">
          <label>
            Provider URL
            <input value={config.baseUrl} onChange={(e) => setConfig({ ...config, baseUrl: e.target.value })} spellCheck={false} />
          </label>
          <div className="presets">
            {PRESETS.map((p) => (
              <button key={p.name} className={preset?.name === p.name ? "seg on" : ""} onClick={() => setConfig({ baseUrl: p.baseUrl, model: p.model ?? config.model })}>{p.name}</button>
            ))}
          </div>
          {preset && <p className="muted small">{preset.note}</p>}
          <label>
            Model
            <input value={config.model} onChange={(e) => setConfig({ ...config, model: e.target.value })} placeholder="e.g. a model from your provider's list" spellCheck={false} />
          </label>
          <label>
            API key for {host}
            <input type="password" value={keyInput} onChange={(e) => setKeyInput(e.target.value)} placeholder={hasKey ? "saved — enter a new one to replace" : "paste key"} autoComplete="off" />
          </label>
          <div className="presets">
            <button onClick={() => void onSaveKey()} disabled={!keyInput.trim()}>Save key</button>
            <button onClick={() => void onClearKey()} disabled={!hasKey}>Remove key</button>
          </div>
          <p className="muted small">
            The key is stored in your operating system's credential store, only for this provider host, and is never
            shown here again. Local servers (localhost) need no key.
          </p>
          {settingsMsg && <p className="note">{settingsMsg}</p>}
        </div>
      )}

      <div className="messages">
        {messages.length === 0 && (
          <p className="muted small">
            Ask about your code. Nothing from the editor is sent unless you choose “Selection” or “Whole file” below and press
            “Send with code”.
          </p>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`msg ${m.role}`}>
            {m.role === "error" && m.error ? (
              <>
                <strong>{ERROR_TITLES[m.error.kind] ?? "Error"}</strong>
                <p>{m.content}</p>
                <div className="row">
                  {(m.error.kind === "missing_key" || m.error.kind === "unauthorized") && (
                    <button onClick={() => setShowSettings(true)}>Open settings</button>
                  )}
                  {m.retry && m.error.kind !== "missing_key" && <button onClick={m.retry} disabled={loading}>{m.retryLabel ?? "Retry"}</button>}
                </div>
              </>
            ) : m.role === "assistant" ? (
              <RichText text={m.content} />
            ) : (
              <>
                <p>{m.content}</p>
                {m.sentLabel && <div className="sentnote">{m.sentLabel}</div>}
              </>
            )}
          </div>
        ))}
        {loading && (
          <div className="msg assistant pending">
            <span className="dots"><i /><i /><i /></span> Waiting for {host}…
          </div>
        )}
        <div ref={endRef} />
      </div>

      <footer className="composer">
        <div className="ctxrow" role="radiogroup" aria-label="Code to include">
          <span className="muted small">Include code:</span>
          <button className={choice === "none" ? "seg on" : "seg"} onClick={() => setChoice("none")}>None</button>
          <button className={choice === "selection" ? "seg on" : "seg"} disabled={!selection} onClick={() => setChoice("selection")}
            title={selection ? "" : "Select some code in the editor first"}>
            Selection{selection ? ` (${selection.endLine - selection.startLine + 1} lines)` : ""}
          </button>
          <button className={choice === "file" ? "seg on" : "seg"} onClick={() => setChoice("file")}>File ({fileLines} lines)</button>
        </div>
        {choice !== "none" && (
          <div className="consent">
            ⚠ {codeChars} characters of code from <strong>{fileName}</strong> will be sent to <strong>{host}</strong> when you
            press “Send with code”.
            {preset?.caution && <div className="caution">{preset.caution}</div>}
          </div>
        )}
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              send();
            }
          }}
          placeholder="Ask about your code… (Ctrl+Enter to send)"
          rows={3}
        />
        <button className="send" onClick={send} disabled={loading || !prompt.trim()}>
          {loading ? "Waiting…" : sendLabel}
        </button>
      </footer>
    </section>
  );
}
