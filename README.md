# Ignytion Python Editor

A small cross-platform desktop editor for Python files, with an AI chat beside
the editor. It runs scripts **only** with the managed Python installed by the
Ignytion Crucible beta installer.

Built for the Ignytion engineering assignment. This is a focused prototype, not an IDE.

**Stack:** Tauri 2 (Rust) · React + TypeScript + Vite · Monaco editor (bundled locally).

## What it does

| Area | Behaviour |
|---|---|
| Edit | New / Open / Save / Save As for `.py` files, Python syntax highlighting, amber dot + "Unsaved changes" indicator, confirm before discarding or closing. |
| Run | Detects the Ignytion-managed Python, shows its **path and version**, runs the current file (saves first), streams **stdout, stderr and exit status**, Stop button. The UI never freezes. |
| Chat | Chat panel beside the editor. Code is sent to the model **only** when you pick "Selection" or "File" and press "Send with code". Loading, failure and missing-key states are shown. |
| Platforms | Windows, macOS, Linux code paths (see [docs/TESTING.md](docs/TESTING.md) for what was actually tested). |

## Critical rule: no silent fallback

Run uses the verified Crucible-managed Python or nothing. If it is missing or
fails verification, Run is **disabled** and the sidebar shows how to install it
(`irm https://get-beta.ignytion.io | iex` on Windows,
`curl -fsSL https://get-beta.ignytion.io/install.sh | bash` on macOS/Linux).
The app never picks `python` from PATH. Details: [docs/RUNTIME-DISCOVERY.md](docs/RUNTIME-DISCOVERY.md).

## Verified on Windows 11

Selected interpreter: `C:\Users\<you>\Crucible\packages\igny_foundation_pkg_win_3.12.10.0\Python\python.exe`,
Python 3.12.10, discovered through the installer's marker file. Run, error output, Stop and
AI chat were all exercised (details in docs/TESTING.md).

## Quick start

Prerequisites: Node.js 20+, Rust (stable), platform build tools for Tauri
(Windows: WebView2 + MSVC Build Tools; Linux: webkit2gtk etc.; macOS: Xcode CLT).
Full steps: [docs/INSTALLATION.md](docs/INSTALLATION.md).

```
npm install
npm run tauri dev
```

## AI configuration

One integration, built as an **OpenAI-compatible adapter** (`/chat/completions`).
The default preset is **Google Gemini free tier** (`gemini-3.5-flash-lite`).
Presets for OpenAI and a local Ollama server are included; any compatible URL works.

1. Get a key from Google AI Studio.
2. In the chat panel open **Settings**, pick **Gemini (free tier)**, paste the key, **Save key**.

Why this choice: one small adapter covers cloud and local models, Gemini has a
free tier that is fast enough for a live demo, and Ollama needs no key at all.

**Key handling:** the key is stored in the OS credential store (Windows
Credential Manager, macOS Keychain, Linux Secret Service) per provider host. It is
read and attached to the request in the Rust layer; the web UI never sees it, it is
not in source, config files or the packaged binary, and it is redacted from errors.
Non-secret settings (URL, model) are in browser local storage.

**Privacy:** on Google's free tier, prompts may be used to improve Google's
products. The consent notice in the app says so. Use Ollama or a paid key for private code.

## Project layout

```
src/                  React UI (App, RuntimePanel, OutputPanel, ChatPanel, services/)
src-tauri/src/
  runtime.rs          managed-Python discovery, verification, process launch
  ai.rs               OpenAI-compatible provider adapter + safety rules
  keystore.rs         OS credential store access
  lib.rs              Tauri commands (thin glue)
docs/                 architecture, discovery, installation, testing
```

## Known limits

- Provenance of the managed Python is checked by the installer's convention
  (marker file, location, version), **not** by a cryptographic signature.
- Stop kills the Python process, not necessarily processes it spawned.
- One file at a time (no tabs), no autocomplete, no input (stdin) for scripts.
- Output is capped (20,000 lines emitted, 5,000 shown) to keep the UI responsive.
- Chat history is in memory only. One provider active at a time.
- Only Windows 11 has been tested with the real Crucible runtime. macOS and Linux are untested (see TESTING.md).
- Not code-signed or packaged as an installer.
