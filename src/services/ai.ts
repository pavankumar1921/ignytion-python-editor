// Thin typed wrappers over the native AI commands. The API key never passes
// through this file: it is stored in, and read from, the OS credential store
// by the Rust layer.
import { invoke } from "@tauri-apps/api/core";

export interface AiConfig {
  baseUrl: string;
  model: string;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface CodeContext {
  kind: "selection" | "file";
  label: string;
  text: string;
}

export interface ChatReply {
  content: string;
  model: string;
  sentCode: boolean;
  sentChars: number;
}

export type AiErrorKind =
  | "missing_key"
  | "consent_required"
  | "invalid_request"
  | "keystore"
  | "network"
  | "timeout"
  | "unauthorized"
  | "rate_limited"
  | "provider"
  | "bad_response"
  | "unknown";

export interface AiError {
  kind: AiErrorKind;
  message: string;
}

export interface Preset {
  name: string;
  baseUrl: string;
  /** model filled in when the preset is chosen (omit to keep what the user typed) */
  model?: string;
  /** one-line help shown in settings */
  note: string;
  /** shown next to the "code will be sent" notice for this provider */
  caution?: string;
}

export const PRESETS: Preset[] = [
  {
    name: "Gemini (free tier)",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai/",
    model: "gemini-3.5-flash-lite",
    note: "Get a key in Google AI Studio. Flash-Lite is the fast option; try gemini-3.8-flash for stronger answers.",
    caution: "On Google's free tier, prompts (including code you send) may be used to improve Google's products.",
  },
  {
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    note: "Needs an API key and a model name from your account.",
  },
  {
    name: "Local (Ollama)",
    baseUrl: "http://localhost:11434/v1",
    note: "Runs on your machine, so nothing leaves it and no key is needed. Type the model you pulled.",
  },
];

const sameUrl = (a: string, b: string) => a.trim().replace(/\/+$/, "") === b.trim().replace(/\/+$/, "");
export const presetFor = (baseUrl: string): Preset | undefined =>
  PRESETS.find((p) => sameUrl(p.baseUrl, baseUrl));

const CONFIG_KEY = "ignytion.ai.config.v1";

export function loadConfig(): AiConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<AiConfig>;
      return { baseUrl: parsed.baseUrl ?? PRESETS[0].baseUrl, model: parsed.model ?? PRESETS[0].model ?? "" };
    }
  } catch {
    /* storage unavailable: fall through to defaults */
  }
  return { baseUrl: PRESETS[0].baseUrl, model: PRESETS[0].model ?? "" };
}

export function saveConfig(cfg: AiConfig): void {
  try {
    localStorage.setItem(CONFIG_KEY, JSON.stringify(cfg)); // non-secret settings only
  } catch {
    /* ignore */
  }
}

/** Native errors arrive as {kind, message}; anything else becomes "unknown". */
export function toAiError(e: unknown): AiError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    const o = e as { kind: unknown; message: unknown };
    return { kind: String(o.kind) as AiErrorKind, message: String(o.message) };
  }
  return { kind: "unknown", message: String(e) };
}

export const hostOf = (baseUrl: string): string => {
  try {
    return new URL(baseUrl).host || baseUrl;
  } catch {
    return baseUrl;
  }
};

export const sendChat = (
  config: AiConfig,
  history: ChatMessage[],
  prompt: string,
  context: CodeContext | null,
  consentToSendCode: boolean,
) =>
  invoke<ChatReply>("ai_chat", {
    request: { config, history, prompt, context, consentToSendCode },
  });

export const keyStatus = (baseUrl: string) => invoke<boolean>("ai_key_status", { baseUrl });
export const saveKey = (baseUrl: string, key: string) => invoke<void>("ai_set_key", { baseUrl, key });
export const clearKey = (baseUrl: string) => invoke<void>("ai_clear_key", { baseUrl });
