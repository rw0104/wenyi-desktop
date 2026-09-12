//! Wenyi Desktop — Rust command layer.
//!
//! Responsibilities:
//! - spawn the `wenyi-core` sidecar (the PyInstaller-packaged `trans-novel` engine),
//! - relay the sidecar's JSONL event stream to the webview frontend,
//! - keep user settings on disk and API keys in the OS credential store,
//! - run the engine in a fixed workspace so `state/` is discoverable for resume,
//! - expose native file dialogs and "open output".
//!
//! The translation engine stays a separate process: long runs are crash-isolated,
//! interruptible and resumable, matching the engine's own checkpoint and lock design.

mod runs;
mod secrets;
mod settings;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// Handle to the running sidecar, so a run can be cancelled.
struct SidecarState(Mutex<Option<CommandChild>>);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest {
    /// translate | prepare | review | assemble | report | status
    command: String,
    /// Absolute path to the input book or subtitle file.
    input: String,
    /// Extra CLI flags forwarded verbatim (e.g. ["--bilingual"]).
    #[serde(default)]
    flags: Vec<String>,
    /// One-off key typed in the UI but not saved; overrides the stored credential.
    #[serde(default)]
    ephemeral_api_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStarted {
    input: String,
    command: String,
}

fn snapshot() -> String {
    // Seconds-resolution UTC timestamp without pulling in a date crate.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{now}")
}

// ── Settings and paths ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_paths(app: AppHandle) -> Result<settings::Paths, String> {
    settings::paths(&app)
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<settings::Settings, String> {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: settings::Settings) -> Result<settings::Paths, String> {
    settings::save(&app, &settings)?;
    settings::paths(&app)
}

// ── API keys (OS credential store) ───────────────────────────────────────────────

#[tauri::command]
fn set_api_key(account: String, secret: String) -> Result<(), String> {
    secrets::set(&account, &secret)
}

#[tauri::command]
fn clear_api_key(account: String) -> Result<(), String> {
    secrets::clear(&account)
}

/// Report which of the requested credentials are present. Secrets never cross the IPC.
#[tauri::command]
fn api_key_status(accounts: Vec<String>) -> std::collections::HashMap<String, bool> {
    accounts
        .into_iter()
        .map(|account| {
            let present = secrets::has(&account);
            (account, present)
        })
        .collect()
}

// ── Resume ───────────────────────────────────────────────────────────────────────

#[tauri::command]
fn list_runs(app: AppHandle) -> Result<Vec<runs::RunSummary>, String> {
    let workspace = settings::workspace_dir(&app)?;
    let config = settings::config_dir(&app)?;
    Ok(runs::list(&workspace, &config))
}

// ── Native dialogs and shell integration ─────────────────────────────────────────

/// Open the native file picker and return an absolute path.
///
/// Implemented on the Rust side so the frontend needs no bundler and no plugin JS globals.
#[tauri::command]
async fn pick_input_file(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<FilePath>>();
    app.dialog()
        .file()
        .set_title("Select a book or subtitle file")
        .add_filter(
            "Books and subtitles",
            &["epub", "fb2", "txt", "md", "html", "pdf", "docx", "srt"],
        )
        .pick_file(move |path| {
            let _ = tx.send(path);
        });

    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| e.to_string())?;

    Ok(picked
        .and_then(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned()))
}

/// Reveal a file or folder in the OS file manager / default application.
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("Path does not exist: {path}"));
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

// ── Engine control ───────────────────────────────────────────────────────────────

/// Kill a running sidecar (cancel an in-flight translation).
#[tauri::command]
async fn cancel(state: State<'_, SidecarState>) -> Result<(), String> {
    if let Some(child) = state.0.lock().unwrap().take() {
        let _ = child.kill();
    }
    Ok(())
}

/// Launch the engine and stream its JSONL events to the frontend.
///
/// The sidecar is built by `sidecar/build_sidecar.ps1` and bundled through
/// `bundle.externalBin`; Tauri resolves the right binary for the host platform.
#[tauri::command]
async fn run_engine(
    app: AppHandle,
    state: State<'_, SidecarState>,
    request: RunRequest,
) -> Result<RunStarted, String> {
    let input = request.input.trim().to_string();
    if input.is_empty() {
        return Err("Select an input file first".into());
    }
    if !PathBuf::from(&input).is_file() {
        return Err(format!("Input file does not exist: {input}"));
    }

    // Cancel any previous run before starting a new one.
    if let Some(child) = state.0.lock().unwrap().take() {
        let _ = child.kill();
    }

    let user_settings = settings::load(&app)?;
    // Keep config.yaml in step with settings even if the UI never pressed Save.
    settings::write_engine_config(&app, &user_settings)?;

    let workspace = settings::workspace_dir(&app)?;
    let config_path = settings::config_file(&app)?;

    // Assemble arguments: <command> <input> [--config PATH] [flags...] --json-events
    let mut args: Vec<String> = vec![request.command.clone(), input.clone()];
    args.push("--config".into());
    args.push(config_path.to_string_lossy().into_owned());
    for flag in request.flags.iter() {
        args.push(flag.clone());
    }
    args.push("--json-events".into());

    // Credentials: a one-off typed key wins, otherwise the OS credential store.
    let mut env: Vec<(String, String)> = Vec::new();
    let key_env = user_settings.api_key_env();
    if !key_env.is_empty() {
        let ephemeral = request
            .ephemeral_api_key
            .as_ref()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        let secret = match ephemeral {
            Some(key) => Some(key),
            None => secrets::get(&key_env),
        };
        match secret {
            Some(key) => env.push((key_env.clone(), key)),
            None if user_settings.requires_api_key() => {
                return Err(format!(
                    "{key_env} is not set. Enter an API key and save it, or use a local model."
                ));
            }
            None => {}
        }
    }

    let proxy = user_settings.proxy.trim().to_string();
    if !proxy.is_empty() {
        env.push(("HTTP_PROXY".into(), proxy.clone()));
        env.push(("HTTPS_PROXY".into(), proxy.clone()));
        env.push(("ALL_PROXY".into(), proxy));
    }

    let sidecar = app.shell().sidecar("wenyi-core").map_err(|e| e.to_string())?;
    let (mut rx, child) = sidecar
        .args(args)
        .envs(env)
        // Run inside the workspace so the engine's relative state/ tree is discoverable.
        .current_dir(workspace)
        .spawn()
        .map_err(|e| format!("Could not start the translation engine: {e}"))?;

    *state.0.lock().unwrap() = Some(child);
    let _ = settings::remember_run(&app, &input, &request.command, &snapshot());

    let started = RunStarted {
        input: input.clone(),
        command: request.command.clone(),
    };
    let _ = app.emit(
        "engine-event",
        serde_json::json!({"event": "started", "input": input, "command": request.command}),
    );

    // Forward the engine's two channels: stdout is machine-readable JSONL, stderr is the
    // human-readable log stream.
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                for raw in text.lines() {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(trimmed) {
                        Ok(value) => {
                            let _ = app.emit("engine-event", value);
                        }
                        Err(_) => {
                            let _ = app.emit(
                                "engine-event",
                                serde_json::json!({"event": "log", "message": raw}),
                            );
                        }
                    }
                }
            }
            CommandEvent::Stderr(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                for raw in text.lines() {
                    if raw.trim().is_empty() {
                        continue;
                    }
                    let _ = app.emit(
                        "engine-event",
                        serde_json::json!({"event": "stderr", "message": raw}),
                    );
                }
            }
            CommandEvent::Terminated(payload) => {
                let _ = app.emit(
                    "engine-event",
                    serde_json::json!({"event": "terminated", "exitCode": payload.code}),
                );
                break;
            }
            _ => {}
        }
    }

    *state.0.lock().unwrap() = None;
    Ok(started)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SidecarState(Mutex::new(None)))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        // Native file drag-and-drop. The drop zone advertises dragging, so it has to
        // actually work — and the hover state must track enter/leave continuously, not
        // only fire on drop.
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Enter { .. })
            | tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Over { .. }) => {
                let _ = window.emit("drag-state", serde_json::json!({"active": true}));
            }
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Leave) => {
                let _ = window.emit("drag-state", serde_json::json!({"active": false}));
            }
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                let _ = window.emit("drag-state", serde_json::json!({"active": false}));
                if let Some(path) = paths.first() {
                    let _ = window.emit(
                        "file-dropped",
                        serde_json::json!({"path": path.to_string_lossy()}),
                    );
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_paths,
            load_settings,
            save_settings,
            set_api_key,
            clear_api_key,
            api_key_status,
            list_runs,
            pick_input_file,
            open_path,
            run_engine,
            cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Wenyi Desktop");
}
