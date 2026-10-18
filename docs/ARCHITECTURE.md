# Architecture

```
┌──────────────────────────── Tauri desktop app ─────────────────────────────┐
│                                                                             │
│  WEB UI (React + Monaco)                  NATIVE LAYER (Rust)               │
│  ┌──────────────────────┐   invoke()     ┌───────────────────────────────┐  │
│  │ Editor, toolbar,     │ ─────────────► │ lib.rs commands               │  │
│  │ runtime panel,       │ ◄───────────── │  read/write_text_file         │  │
│  │ output panel,        │  run-output    │  get_runtime_status           │  │
│  │ chat panel           │  events        │  select_interpreter           │  │
│  └──────────────────────┘                │  run_python / stop_python     │  │
│            ▲  never holds the API key    │  ai_chat / ai_*_key           │  │
│            │                             └──────┬───────────┬────────────┘  │
│            │                                    │           │               │
└────────────┼────────────────────────────────────┼───────────┼───────────────┘
             │                                    │           │
   BOUNDARY 1: UI ⇄ native              BOUNDARY 2: │         │ BOUNDARY 3:
   typed commands only                  PYTHON PROCESS         AI PROVIDER
                                                    ▼           ▼
                                   ┌───────────────────┐  ┌──────────────────────┐
                                   │ Verified Ignytion │  │ HTTPS to provider    │
                                   │ managed python -u │  │ (Gemini / OpenAI /   │
                                   │ <saved script>    │  │ local Ollama)        │
                                   │ child process,    │  │ key from OS keystore │
                                   │ no shell          │  └──────────────────────┘
                                   └───────────────────┘
```

## The three boundaries

1. **UI ⇄ native.** The UI can only call named Tauri commands. It cannot spawn
   processes, read arbitrary files or see secrets.
2. **Native ⇄ Python process.** Only the interpreter stored in app state, which is
   set solely by successful verification, can be launched. Arguments are an array
   (no shell). stdout and stderr are read concurrently and emitted as events; the
   UI batches them per animation frame, so a noisy script cannot freeze the editor.
3. **Native ⇄ AI provider.** One adapter (`ai.rs`). The key comes from the OS
   credential store inside Rust. Code is attached only if the request carries
   `consent_to_send_code`, which the UI sets only from the "Send with code" button.
   Consent is enforced in Rust as well as the UI.

## Design decisions

- **Child process, async everywhere.** Run, stop and chat are async commands; the
  UI thread never waits on Python or the network.
- **Run saves first.** The file on disk is what runs; an unsaved buffer never runs.
- **No silent switch.** Discovery returns either a verified interpreter or a
  reasoned failure with install instructions. There is no PATH search.
- **OpenAI-compatible adapter.** The `/chat/completions` shape is spoken by
  Gemini, OpenAI, Ollama, Groq and others, so one adapter gives provider choice
  without extra code. Trade-off: provider-specific features are not used.
- **Keys in the OS credential store**, per provider host; redacted from errors.
- **Typed errors** (`missing_key`, `consent_required`, `network`, `timeout`,
  `unauthorized`, `rate_limited`, `provider`, `bad_response`, ...) map to
  distinct, readable cards in the chat.
- **Modules without Tauri dependencies** (`runtime.rs`, `ai.rs`) so the safety
  rules are unit-testable.

## Safety limits

Code context ≤ 60,000 chars; history ≤ 20 messages; 10 s connect / 60 s request
timeout; plain `http://` refused for non-local hosts; output capped.
