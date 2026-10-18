# Installation

## 1. Install the Ignytion managed Python (Crucible beta)

Follow the installer's agreement flow.

- Windows PowerShell: `irm https://get-beta.ignytion.io | iex`
- macOS / Linux: `curl -fsSL https://get-beta.ignytion.io/install.sh | bash`

The editor reads the result; it does not modify the installation.

## 2. Build prerequisites

- Node.js 20+ and npm
- Rust stable (`rustup`)
- **Windows:** Microsoft C++ Build Tools, WebView2 (included in Windows 11)
- **macOS:** Xcode Command Line Tools
- **Linux:** webkit2gtk-4.1, libssl, build-essential and the other packages listed in the Tauri 2 prerequisites guide

## 3. Run

```
npm install
npm run tauri dev
```

First build compiles Rust dependencies and takes several minutes.

## 4. Configure AI (optional but expected)

See "AI configuration" in the README. Local alternative: install Ollama, pull a
model, choose the **Local (Ollama)** preset, set the model name. No key needed.

## Development override

`IGNYTION_PYTHON_OVERRIDE=<path>` points the editor at a specific interpreter. It
must pass the **same verification** (inside the Ignytion installation, `--version`
check). An invalid override does not fall back to anything.

## Paths with spaces

Scripts and interpreters are launched with argument arrays, never through a shell,
so spaces in paths are safe.
