//! Ignytion managed-Python discovery, verification and execution.
//!
//! This module has no Tauri dependency on purpose: the trust decisions
//! ("is this interpreter really the Ignytion-managed one?") are plain
//! functions that can be unit-tested without a window.
//!
//! Trust chain (mirrors the beta installers' own logic, see
//! docs/RUNTIME-DISCOVERY.md):
//!   1. `<home>/Crucible/.sandbox/current_foundation_pkg_dir.txt` exists
//!      (written by the Foundation installer).
//!   2. Its first line names the Foundation package directory.
//!   3. Only the installer's own candidate paths inside that directory are tried.
//!   4. The candidate's *canonical* path must be inside the package directory.
//!   5. `<candidate> --version` must run and report `Python 3.x`.
//! Nothing here ever consults PATH or falls back to another interpreter.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

const MAX_EMITTED_LINES: usize = 20_000;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------- types

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub verified: bool,
    pub python_path: Option<String>,
    pub version: Option<String>,
    pub package_dir: Option<String>,
    /// "marker" | "user-selected" | "dev-override"
    pub source: String,
    pub message: String,
    pub checks: Vec<Check>,
}

#[derive(Clone, Debug)]
pub struct VerifiedRuntime {
    pub python: PathBuf,
    pub version: String,
    pub package_dir: PathBuf,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    /// "stdout" | "stderr"
    pub stream: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub exit_code: Option<i32>,
    pub killed: bool,
    pub duration_ms: u64,
    /// Process-start / validation errors (not Python tracebacks, which are stderr).
    pub error: Option<String>,
}

// ---------------------------------------------------------------- paths

pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

pub fn marker_path(home: &Path) -> PathBuf {
    home.join("Crucible")
        .join(".sandbox")
        .join("current_foundation_pkg_dir.txt")
}

/// The installers' candidate list. On Windows the real interpreter is tried before
/// the `bin\python.bat` wrapper: the wrapper only forwards to `Python\python.exe`
/// (confirmed on a real install), and killing a wrapper on Stop would leave Python running.
fn candidates(pkg: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            pkg.join("bin").join("python.exe"),
            pkg.join("Python").join("python.exe"),
            pkg.join("bin").join("python.bat"),
        ]
    } else {
        vec![
            pkg.join("bin").join("python"),
            pkg.join("bin").join("python3"),
            pkg.join("Python").join("python"),
        ]
    }
}

fn is_runnable_file(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Windows canonical paths carry a `\\?\` prefix; strip it for display.
pub fn display(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => s,
    }
}

fn push(checks: &mut Vec<Check>, name: &str, ok: bool, detail: impl Into<String>) {
    checks.push(Check {
        name: name.to_string(),
        ok,
        detail: detail.into(),
    });
}

fn install_hint() -> String {
    "Install the Ignytion Crucible beta runtime, then press Re-detect.\n\
     Windows PowerShell:  irm https://get-beta.ignytion.io | iex\n\
     macOS / Linux:       curl -fsSL https://get-beta.ignytion.io/install.sh | bash\n\
     Or choose the managed interpreter manually with \"Select interpreter…\"."
        .to_string()
}

fn failure(
    source: &str,
    headline: &str,
    checks: Vec<Check>,
    package_dir: Option<&Path>,
) -> (RuntimeStatus, Option<VerifiedRuntime>) {
    (
        RuntimeStatus {
            verified: false,
            python_path: None,
            version: None,
            package_dir: package_dir.map(display),
            source: source.to_string(),
            message: format!("{headline}\n\n{}", install_hint()),
            checks,
        },
        None,
    )
}

// ---------------------------------------------------------------- probing

/// Runs `<python> --version` (full path, argument array, no shell) with a timeout.
async fn probe_version(python: &Path) -> Result<String, String> {
    let mut cmd = Command::new(python);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let out = match tokio::time::timeout(PROBE_TIMEOUT, cmd.output()).await {
        Err(_) => return Err("timed out after 10 s".into()),
        Ok(Err(e)) => return Err(format!("could not start: {e}")),
        Ok(Ok(o)) => o,
    };
    if !out.status.success() {
        return Err(format!("exited with {}", out.status));
    }
    // Python 3 prints to stdout; very old versions used stderr. Accept either.
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let line = text.lines().next().unwrap_or("").trim().to_string();
    if line.starts_with("Python 3") {
        Ok(line)
    } else {
        Err(format!("unexpected --version output: {line:?}"))
    }
}

/// Containment + version probe shared by auto-discovery and manual selection.
async fn verify_candidate(
    candidate: &Path,
    roots: &[PathBuf],
    checks: &mut Vec<Check>,
) -> Result<(String, PathBuf), String> {
    let resolved = std::fs::canonicalize(candidate).map_err(|e| {
        push(checks, "Resolve interpreter path", false, e.to_string());
        format!("Cannot resolve {}: {e}", display(candidate))
    })?;
    push(checks, "Resolve interpreter path", true, display(&resolved));

    let root = roots.iter().find(|r| resolved.starts_with(r));
    match root {
        None => {
            let allowed = roots
                .iter()
                .map(|r| display(r))
                .collect::<Vec<_>>()
                .join(", ");
            let detail = format!("{} is outside the Ignytion installation ({allowed})", display(&resolved));
            push(checks, "Inside Ignytion installation", false, detail.clone());
            Err(detail)
        }
        Some(root) => {
            push(
                checks,
                "Inside Ignytion installation",
                true,
                format!("under {}", display(root)),
            );
            match probe_version(candidate).await {
                Ok(v) => {
                    push(checks, "Version probe (--version)", true, v.clone());
                    Ok((v, root.clone()))
                }
                Err(e) => {
                    push(checks, "Version probe (--version)", false, e.clone());
                    Err(format!("Version probe failed: {e}"))
                }
            }
        }
    }
}

fn read_marker_package_dir(home: &Path, checks: &mut Vec<Check>) -> Result<PathBuf, String> {
    let marker = marker_path(home);
    let raw = std::fs::read_to_string(&marker).map_err(|e| {
        push(
            checks,
            "Foundation marker file",
            false,
            format!("{} — {e}", display(&marker)),
        );
        "The Ignytion Foundation marker file was not found, so the Crucible beta runtime does not appear to be installed.".to_string()
    })?;
    push(checks, "Foundation marker file", true, display(&marker));

    // PowerShell may write a BOM; the installers may quote the path.
    let first = raw
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches('\u{feff}')
        .trim()
        .trim_matches('"')
        .trim();
    if first.is_empty() {
        push(checks, "Package directory", false, "marker file is empty");
        return Err("The Foundation marker file is empty.".into());
    }
    let pkg = std::fs::canonicalize(first).map_err(|e| {
        push(checks, "Package directory", false, format!("{first} — {e}"));
        format!("The Foundation package directory named by the marker does not exist: {first}")
    })?;
    if !pkg.is_dir() {
        push(checks, "Package directory", false, format!("{} is not a directory", display(&pkg)));
        return Err("The Foundation package path is not a directory.".into());
    }
    push(checks, "Package directory", true, display(&pkg));
    Ok(pkg)
}

// ---------------------------------------------------------------- discovery

/// Automatic discovery via the Foundation marker file.
pub async fn discover(home: &Path) -> (RuntimeStatus, Option<VerifiedRuntime>) {
    let mut checks = Vec::new();
    let pkg = match read_marker_package_dir(home, &mut checks) {
        Ok(p) => p,
        Err(headline) => return failure("marker", &headline, checks, None),
    };

    let tried = candidates(&pkg);
    let Some(candidate) = tried.iter().find(|c| is_runnable_file(c)) else {
        push(
            &mut checks,
            "Interpreter candidate",
            false,
            format!(
                "none of: {}",
                tried.iter().map(|c| display(c)).collect::<Vec<_>>().join(", ")
            ),
        );
        return failure(
            "marker",
            "The Foundation package exists but contains no managed Python executable.",
            checks,
            Some(&pkg),
        );
    };
    push(&mut checks, "Interpreter candidate", true, display(candidate));
    if candidate.extension().and_then(|e| e.to_str()) == Some("bat") {
        push(
            &mut checks,
            "Launcher type",
            true,
            "python.bat wrapper (the installer's fallback candidate)",
        );
    }

    let roots = vec![pkg.clone()];
    match verify_candidate(candidate, &roots, &mut checks).await {
        Err(e) => failure("marker", &e, checks, Some(&pkg)),
        Ok((version, root)) => {
            // Informational corroboration; never gates trust.
            let license = home.join(".ignytion").join("license.json");
            push(
                &mut checks,
                "Ignytion license file (informational)",
                license.is_file(),
                display(&license),
            );
            let rt = VerifiedRuntime {
                python: candidate.clone(),
                version: version.clone(),
                package_dir: root,
            };
            (
                RuntimeStatus {
                    verified: true,
                    python_path: Some(display(candidate)),
                    version: Some(version),
                    package_dir: Some(display(&rt.package_dir)),
                    source: "marker".into(),
                    message: "Using the Ignytion-managed Python installed by the Crucible beta installer."
                        .into(),
                    checks,
                },
                Some(rt),
            )
        }
    }
}

/// Manual selection (or dev override): the interpreter must still live inside
/// the Ignytion installation. Allowed roots are the marker's package directory
/// and `<home>/Crucible`.
pub async fn verify_selected(
    home: &Path,
    selected: &Path,
    source: &str,
) -> (RuntimeStatus, Option<VerifiedRuntime>) {
    let mut checks = Vec::new();

    let mut roots: Vec<PathBuf> = Vec::new();
    let mut scratch = Vec::new();
    if let Ok(pkg) = read_marker_package_dir(home, &mut scratch) {
        roots.push(pkg);
    }
    if let Ok(c) = std::fs::canonicalize(home.join("Crucible")) {
        roots.push(c);
    }
    if roots.is_empty() {
        push(
            &mut checks,
            "Ignytion installation",
            false,
            "no Foundation marker and no Crucible directory found",
        );
        return failure(
            source,
            "No Ignytion installation was found, so no interpreter can be verified as managed.",
            checks,
            None,
        );
    }

    let name_ok = selected
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            let s = s.to_ascii_lowercase();
            s.starts_with("python") && s != "pythonw"
        })
        .unwrap_or(false);
    push(
        &mut checks,
        "Looks like a Python interpreter",
        name_ok,
        display(selected),
    );
    if !name_ok {
        return failure(source, "The selected file is not a Python interpreter (expected python / python3 / python.exe).", checks, None);
    }
    if !is_runnable_file(selected) {
        push(&mut checks, "File is runnable", false, display(selected));
        return failure(source, "The selected file does not exist or is not executable.", checks, None);
    }

    match verify_candidate(selected, &roots, &mut checks).await {
        Err(e) => failure(source, &format!("Rejected: {e}"), checks, None),
        Ok((version, root)) => {
            let rt = VerifiedRuntime {
                python: selected.to_path_buf(),
                version: version.clone(),
                package_dir: root,
            };
            (
                RuntimeStatus {
                    verified: true,
                    python_path: Some(display(selected)),
                    version: Some(version),
                    package_dir: Some(display(&rt.package_dir)),
                    source: source.to_string(),
                    message: "Using the interpreter you selected (verified inside the Ignytion installation)."
                        .into(),
                    checks,
                },
                Some(rt),
            )
        }
    }
}

/// Entry point used by the app: dev override if set (validated, no silent
/// fallback when it is invalid), otherwise marker discovery.
pub async fn resolve(home: Option<PathBuf>) -> (RuntimeStatus, Option<VerifiedRuntime>) {
    let Some(home) = home else {
        return failure(
            "marker",
            "Could not determine your home directory.",
            vec![],
            None,
        );
    };
    if let Ok(p) = std::env::var("IGNYTION_PYTHON_OVERRIDE") {
        if !p.trim().is_empty() {
            return verify_selected(&home, Path::new(p.trim()), "dev-override").await;
        }
    }
    discover(&home).await
}

// ---------------------------------------------------------------- execution

fn spawn_reader<R, F>(
    reader: Option<R>,
    stream: &'static str,
    cb: Arc<F>,
    emitted: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
    F: Fn(RunOutput) + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let Some(reader) = reader else { return };
        let mut rd = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match rd.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let n = emitted.fetch_add(1, Ordering::Relaxed);
                    if n < MAX_EMITTED_LINES {
                        let text = String::from_utf8_lossy(&buf)
                            .trim_end_matches(['\r', '\n'])
                            .to_string();
                        cb(RunOutput { stream: stream.into(), text });
                    } else if n == MAX_EMITTED_LINES {
                        cb(RunOutput {
                            stream: "stderr".into(),
                            text: format!("[output truncated after {MAX_EMITTED_LINES} lines]"),
                        });
                    }
                    // beyond the cap: keep draining so the child never blocks on a full pipe
                }
            }
        }
    })
}

/// Runs a saved `.py` file with the verified interpreter.
/// Arguments are passed as an array — never through a shell.
pub async fn run_script<F>(
    rt: &VerifiedRuntime,
    script: &Path,
    mut kill_rx: oneshot::Receiver<()>,
    on_output: F,
) -> RunResult
where
    F: Fn(RunOutput) + Send + Sync + 'static,
{
    let started = Instant::now();
    let fail = |msg: String| RunResult {
        exit_code: None,
        killed: false,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some(msg),
    };

    match std::fs::metadata(script) {
        Ok(m) if m.is_file() => {}
        Ok(_) => return fail(format!("{} is not a file.", display(script))),
        Err(e) => return fail(format!("Cannot read {}: {e}", display(script))),
    }
    let is_py = script
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("py"))
        .unwrap_or(false);
    if !is_py {
        return fail("Only .py files can be run.".into());
    }
    let script_abs = match std::path::absolute(script) {
        Ok(p) => p,
        Err(e) => return fail(format!("Cannot resolve script path: {e}")),
    };

    let mut cmd = Command::new(&rt.python);
    cmd.arg("-u")
        .arg(&script_abs)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    if let Some(dir) = script_abs.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return fail(format!(
                "Could not start the managed Python at {}: {e}",
                display(&rt.python)
            ))
        }
    };

    let cb = Arc::new(on_output);
    let emitted = Arc::new(AtomicUsize::new(0));
    let out_task = spawn_reader(child.stdout.take(), "stdout", cb.clone(), emitted.clone());
    let err_task = spawn_reader(child.stderr.take(), "stderr", cb.clone(), emitted.clone());

    let mut killed = false;
    let status = tokio::select! {
        s = child.wait() => s,
        Ok(()) = &mut kill_rx => {
            killed = true;
            let _ = child.kill().await;
            child.wait().await
        }
    };

    // Readers end when the pipes close; don't hang if a grandchild holds them open.
    let _ = tokio::time::timeout(Duration::from_secs(2), out_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), err_task).await;

    let duration_ms = started.elapsed().as_millis() as u64;
    match status {
        Ok(st) => RunResult {
            exit_code: st.code(),
            killed,
            duration_ms,
            error: None,
        },
        Err(e) => RunResult {
            exit_code: None,
            killed,
            duration_ms,
            error: Some(format!("Failed while waiting for the process: {e}")),
        },
    }
}
