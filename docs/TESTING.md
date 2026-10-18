# Testing and platform evidence

## Platforms

| Platform | Status | Evidence |
|---|---|---|
| Windows 11 | **Tested end to end** (editor, managed Python, chat) | Results below |
| macOS | **Not tested** | No access. Code paths exist; unverified. |
| Linux | **Partly tested** | Rust unit tests and UI tests ran on Linux (sandbox); the desktop app itself was not launched. |

## Automated checks (run during development)

- Rust: runtime discovery against a fake managed-Python tree (marker parsing,
  containment, symlink escape, version probe, no-fallback, run/stop/exit codes,
  paths with spaces); AI adapter against a mock HTTP server (consent, missing key,
  HTTP-to-remote refusal, key redaction, 401/429/5xx, timeout, bad JSON,
  trailing-slash URL). 15 AI tests passing.
- TypeScript: `tsc --strict` clean; `vite build` succeeds.
- UI: 31 headless-browser checks (Playwright, stubbed backend) covering
  consent flow, error cards, loading state, selection/file context.

These do **not** replace testing on a real Crucible install.

## Manual test checklist (Windows 11)

| # | Test | Expected | Result |
|---|---|---|---|
| A | Run starter script | Output "Hello from Ignytion!", exit status 0 | **Passed** (printed `Hello from Ignytion!` and `30`) |
| B | Run `1/0` | ZeroDivisionError on stderr, non-zero exit | **Passed** (traceback on stderr, "Exit status 1", 0.05 s) |
| C | Chat: select code, Send with code | Real answer | **Passed** (Gemini free tier) |
| D | Runtime missing | Run disabled, install message | **Passed** (observed before Crucible install: "runtime not verified") |
| E | Chat failure | Missing key card; connect error with `http://localhost:9/v1` | **Passed** (missing-key card; "Network problem: Could not connect to localhost" with Retry) |
| F | Stop a long-running script | Stops, status "Stopped" | **Passed** (stopped after `tick 3`, 3.61 s, chip "Stopped") |
| G | Path with spaces | Runs | **Passed** (`spaced script.py` exit 0; also re-run from a folder named `My Test Folder`) |
| H | Unsaved indicator, close prompt | Amber dot; confirm dialog | **Passed** (amber dot; closing with unsaved changes shows a confirm prompt) |

Also observed: a script with an `IndentationError` (accidental leading space) showed
its error on stderr, so run errors display for syntax errors as well as runtime errors.

## The real runtime (Windows 11, Crucible beta installer, Crucible 0.0.7b95)

- Marker file `C:\Users\vasuk\Crucible\.sandbox\current_foundation_pkg_dir.txt`
  contains `C:\Users\vasuk\Crucible\packages\igny_foundation_pkg_win_3.12.10.0`.
- Candidates present: `bin\python.exe` **no**, `bin\python.bat` yes, `Python\python.exe` yes.
- Selected: `...\igny_foundation_pkg_win_3.12.10.0\Python\python.exe`, **Python 3.12.10**,
  found via "marker". All verification checks passed.
- `bin\python.bat` is a wrapper that forwards to `..\python\python.exe`.

Not run on the real runtime: no tests of a missing-marker or tampered install were
done on a real machine (covered only by the simulated tests above).
