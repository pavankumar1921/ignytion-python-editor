# Runtime discovery and verification

Goal: use the Python installed by the Ignytion Crucible beta installer, and
**record how we know it belongs to that installation**.

## Trust chain

1. **Marker file.** The installer records the Foundation package location in
   `<home>/Crucible/.sandbox/current_foundation_pkg_dir.txt`. The first line is
   the package directory (BOM and quotes stripped).
2. **Candidate interpreters inside that directory:**
   - Windows (in this order): `bin\python.exe`, `Python\python.exe`, `bin\python.bat`.
     The `.bat` is a wrapper that only forwards to `Python\python.exe`; it is tried last
     because stopping a wrapper would not stop Python itself.
   - macOS / Linux: `bin/python`, `bin/python3`, `Python/python`
3. **Containment.** The candidate's canonical (symlink-resolved) path must be inside
   the package directory. Symlinks pointing outside are rejected.
4. **Version probe.** `<python> --version` must succeed within 10 s and print
   `Python 3.x`.
5. **Informational:** license metadata is shown if present.

Only if every step passes is the interpreter stored. The UI shows each check
(pass/fail with detail) under "Verification".

## Manual selection

The user may pick an interpreter, but it must pass the same verification and sit
inside the marker's package directory or `<home>/Crucible`. A rejected pick leaves
the current runtime unchanged and explains why. A random system Python is rejected.

## Missing runtime

Run is disabled. The sidebar shows the failed check and both installer commands,
plus Re-detect and Select buttons.

## Limits (honest)

- Provenance is by installer convention and location, **not** a signature or
  hash check. Anyone who can write into the Crucible directory can pass it.
- Marker-file format was taken from the installer scripts; it may change in later
  beta builds.
- Verified on one real install (Windows 11, Crucible 0.0.7b95): marker -> package dir
  `igny_foundation_pkg_win_3.12.10.0` -> `Python\python.exe` -> Python 3.12.10. See TESTING.md.
  macOS/Linux discovery follows the installer scripts but has not been run on those systems.

## Platform notes

Home is resolved from `USERPROFILE` (Windows) or `HOME`. On Windows the process is
started with `CREATE_NO_WINDOW`; `PYTHONUTF8=1` is set so output is UTF-8.
