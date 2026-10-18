mod ai;
mod keystore;
mod runtime;

use runtime::{RunOutput, RunResult, RuntimeStatus, VerifiedRuntime};
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

/// The single verified interpreter (if any) and the kill switch of the running script.
/// Run only ever reads `runtime`; it is set exclusively by successful verification.
#[derive(Default)]
struct AppState {
    runtime: Mutex<Option<VerifiedRuntime>>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
}

// ------------------------------------------------------------------ files

#[tauri::command]
async fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Could not read '{path}': {e}"))
}

#[tauri::command]
async fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("Could not write '{path}': {e}"))
}

// ------------------------------------------------------------------ runtime

/// (Re-)detect the managed runtime. Replaces the stored runtime with the result,
/// so a runtime that disappeared or became invalid disables Run.
#[tauri::command]
async fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    let (status, rt) = runtime::resolve(runtime::home_dir()).await;
    *state.runtime.lock().unwrap() = rt;
    Ok(status)
}

/// Manual selection. The interpreter must pass the same verification
/// (inside the Ignytion installation + `--version`). A rejected pick leaves the
/// current runtime untouched and is reported back to the UI.
#[tauri::command]
async fn select_interpreter(
    state: State<'_, AppState>,
    path: String,
) -> Result<RuntimeStatus, String> {
    let Some(home) = runtime::home_dir() else {
        return Err("Could not determine your home directory.".into());
    };
    let (status, rt) = runtime::verify_selected(&home, Path::new(&path), "user-selected").await;
    if let Some(rt) = rt {
        *state.runtime.lock().unwrap() = Some(rt);
    }
    Ok(status)
}

// ------------------------------------------------------------------ run

/// Runs a *saved* .py file with the verified interpreter. Output streams to the UI
/// as `run-output` events; the final status is the command's return value.
#[tauri::command]
async fn run_python(
    app: AppHandle,
    state: State<'_, AppState>,
    script_path: String,
) -> Result<RunResult, String> {
    let rt = state.runtime.lock().unwrap().clone().ok_or_else(|| {
        "No verified Ignytion Python runtime. Run is disabled until the managed runtime is \
         detected or you select a managed interpreter."
            .to_string()
    })?;

    let (tx, rx) = oneshot::channel();
    {
        let mut kill = state.kill.lock().unwrap();
        if kill.is_some() {
            return Err("A script is already running. Stop it first.".into());
        }
        *kill = Some(tx);
    }

    let emitter = app.clone();
    let result = runtime::run_script(&rt, Path::new(&script_path), rx, move |out: RunOutput| {
        let _ = emitter.emit("run-output", out);
    })
    .await;

    *state.kill.lock().unwrap() = None;
    Ok(result)
}

#[tauri::command]
async fn stop_python(state: State<'_, AppState>) -> Result<bool, String> {
    let tx = state.kill.lock().unwrap().take();
    Ok(match tx {
        Some(tx) => tx.send(()).is_ok(),
        None => false,
    })
}

// ------------------------------------------------------------------ AI chat

/// The web frontend never sees the API key: it is read from the OS credential
/// store here and attached to the provider request in the native layer.
#[tauri::command]
async fn ai_chat(request: ai::ChatRequest) -> Result<ai::ChatReply, ai::AiError> {
    let url = request.config.base_url.clone();
    let key = match keystore::get(&url) {
        Ok(k) => k,
        Err(e) => {
            // A local model needs no key, so a broken keystore must not block it.
            let local = ai::host_of(&url).map(|h| ai::is_local_host(&h)).unwrap_or(false);
            if local {
                None
            } else {
                return Err(ai::AiError::Keystore(e));
            }
        }
    };
    ai::chat(&request, key.as_deref()).await
}

#[tauri::command]
async fn ai_key_status(base_url: String) -> Result<bool, String> {
    Ok(keystore::get(&base_url)?.is_some())
}

#[tauri::command]
async fn ai_set_key(base_url: String, key: String) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("The API key is empty.".into());
    }
    keystore::set(&base_url, key)
}

#[tauri::command]
async fn ai_clear_key(base_url: String) -> Result<(), String> {
    keystore::clear(&base_url)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            write_text_file,
            get_runtime_status,
            select_interpreter,
            run_python,
            stop_python,
            ai_chat,
            ai_key_status,
            ai_set_key,
            ai_clear_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ignytion Python Editor");
}
