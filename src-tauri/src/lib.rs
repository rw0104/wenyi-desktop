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

/// Credential name the engine reads for PDF conversion. Kept separate from the LLM
/// provider keys because MinerU is not a translation provider: it is the external OCR and
/// layout service the engine uses for PDF input, with its own account and its own key.
pub const MINERU_KEY_ACCOUNT: &str = "MINERU_API_KEY";

#[tauri::command]
fn get_effective_config(settings: settings::Settings) -> settings::EffectiveConfig {
    settings::describe(&settings)
}

#[tauri::command]
fn list_runs(app: AppHandle) -> Result<Vec<runs::RunSummary>, String> {
    let workspace = settings::workspace_dir(&app)?;
    let config = settings::config_dir(&app)?;
    Ok(runs::list(&workspace, &config))
}

/// Build the environment the engine runs with: credentials and proxy, never on the
/// command line (a process list would otherwise leak the key).
///
/// Shared by a real run and the connection test so both behave identically.
fn engine_env(
    user_settings: &settings::Settings,
    ephemeral_api_key: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    let mut env: Vec<(String, String)> = Vec::new();

    let key_env = user_settings.api_key_env();
    if !key_env.is_empty() {
        let ephemeral = ephemeral_api_key
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

    // PDF input is converted by MinerU, which uses its own key. Without this the run dies
    // at "Parsing document..." with a message naming an environment variable the user has
    // no way to set, which reads as "the model key is broken".
    if let Some(mineru_key) = secrets::get(MINERU_KEY_ACCOUNT) {
        env.push((MINERU_KEY_ACCOUNT.into(), mineru_key));
    }

    Ok(env)
}

/// Outcome of a connection test, in language a user can act on.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestResult {
    ok: bool,
    message: String,
}

/// Run one short paragraph through the real pipeline to prove the key and endpoint work.
///
/// Deliberately not a metadata ping: the engine needs a JSON-capable chat completion, so
/// the only honest test is an actual translation. It uses a throwaway workspace, so the
/// user's state directory is untouched, and it costs a handful of tokens.
#[tauri::command]
async fn test_connection(
    app: AppHandle,
    ephemeral_api_key: Option<String>,
) -> Result<TestResult, String> {
    let user_settings = settings::load(&app)?;
    let env = engine_env(&user_settings, ephemeral_api_key.as_deref())?;

    let dir = std::env::temp_dir().join("wenyi-connection-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let config_path = dir.join("config.yaml");
    std::fs::write(&config_path, settings::render_config_yaml(&user_settings))
        .map_err(|e| e.to_string())?;

    let book_path = dir.join("connection-probe.txt");
    std::fs::write(
        &book_path,
        "Chapter One\n\nThe harbour was quiet at dawn, and the boats did not move.\n",
    )
    .map_err(|e| e.to_string())?;

    let args = build_engine_args(
        "translate",
        &book_path.to_string_lossy(),
        &config_path.to_string_lossy(),
        &["--no-polish".to_string(), "--no-review".to_string()],
    );

    let sidecar = app.shell().sidecar("wenyi-core").map_err(|e| e.to_string())?;
    let (mut rx, child) = sidecar
        .args(args)
        .envs(env)
        .current_dir(&dir)
        .spawn()
        .map_err(|e| format!("Could not start the translation engine: {e}"))?;

    let mut result = TestResult {
        ok: false,
        message: "The engine stopped without reporting a result.".into(),
    };
    let mut finished = false;

    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                for raw in text.lines() {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                        continue;
                    };
                    match value.get("event").and_then(|v| v.as_str()) {
                        Some("done") => {
                            result = TestResult {
                                ok: true,
                                message: "Connected: the model answered and JSON was parsed."
                                    .into(),
                            };
                            finished = true;
                        }
                        Some("error") => {
                            let detail = value
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error");
                            result = TestResult {
                                ok: false,
                                message: explain_engine_error(detail),
                            };
                            finished = true;
                        }
                        _ => {}
                    }
                }
            }
            CommandEvent::Stderr(bytes) => {
                // Keep the last meaningful stderr line: it carries errors that never
                // reached the event stream (for example a startup failure).
                let text = String::from_utf8_lossy(&bytes);
                for raw in text.lines() {
                    if let Some(rest) = raw.trim().strip_prefix("Error: ") {
                        if !finished {
                            result.message = explain_engine_error(rest);
                        }
                    }
                }
            }
            CommandEvent::Terminated(_) => break,
            _ => {}
        }
        if finished {
            let _ = child.kill();
            break;
        }
    }

    Ok(result)
}

/// Turn an engine error into something the user can act on.
///
/// The raw text is written for a developer reading a terminal; several of these name an
/// environment variable the desktop shell owns, which is exactly the case where the
/// message must point at a control in the app instead.
fn explain_engine_error(detail: &str) -> String {
    let lower = detail.to_lowercase();
    if lower.contains("mineru_api_key") || lower.contains("api token not provided") {
        return format!(
            "PDF input needs a MinerU key, which is separate from the translation model \
             key. Add it under Settings. (engine: {detail})"
        );
    }
    if lower.contains("401") || lower.contains("authentication") || lower.contains("invalid api key")
    {
        return format!(
            "The endpoint rejected the API key. Check that the key belongs to the \
             endpoint shown above. (engine: {detail})"
        );
    }
    if lower.contains("model not exist") || lower.contains("model_not_found") {
        return format!(
            "The endpoint does not recognise the configured model id. (engine: {detail})"
        );
    }
    if lower.contains("connection") || lower.contains("timeout") || lower.contains("dns") {
        return format!(
            "Could not reach the endpoint. If your network needs a proxy, set it under \
             Settings. (engine: {detail})"
        );
    }
    if lower.contains("no module named") {
        return format!("The bundled engine is incomplete: {detail}");
    }
    detail.to_string()
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

/// Build the engine command line.
///
/// Order matters: `--config` and `--json-events` are *group-level* options declared on the
/// CLI's root callback, so they must precede the subcommand. Command-level switches such as
/// `--no-polish` must follow it. Getting this wrong makes every invocation fail with
/// "No such option: --config", which no compile-time check can catch — hence the test below.
fn build_engine_args(
    command: &str,
    input: &str,
    config_path: &str,
    flags: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = Vec::with_capacity(6 + flags.len());
    args.push("--config".into());
    args.push(config_path.to_string());
    args.push("--json-events".into());
    // Subcommand and its positional argument.
    args.push(command.to_string());
    args.push(input.to_string());
    // Command-level flags belong after the subcommand.
    args.extend(flags.iter().cloned());
    args
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

    let args = build_engine_args(
        &request.command,
        &input,
        &config_path.to_string_lossy(),
        &request.flags,
    );

    let env = engine_env(&user_settings, request.ephemeral_api_key.as_deref())?;

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
            get_effective_config,
            set_api_key,
            clear_api_key,
            api_key_status,
            list_runs,
            pick_input_file,
            open_path,
            run_engine,
            test_connection,
            cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Wenyi Desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `--config` and `--json-events` used to be appended after the subcommand,
    /// which the engine rejects with "No such option: --config" on every run. Group-level
    /// options must come first; command-level flags must come after.
    #[test]
    fn group_options_precede_the_subcommand_and_flags_follow_it() {
        let flags = vec!["--no-polish".to_string(), "--bilingual".to_string()];
        let args = build_engine_args("translate", "book.epub", "C:\\ws\\config.yaml", &flags);

        assert_eq!(
            args,
            vec![
                "--config",
                "C:\\ws\\config.yaml",
                "--json-events",
                "translate",
                "book.epub",
                "--no-polish",
                "--bilingual",
            ]
        );

        let subcommand = args.iter().position(|a| a == "translate").unwrap();
        for group_option in ["--config", "--json-events"] {
            let pos = args.iter().position(|a| a == group_option).unwrap();
            assert!(
                pos < subcommand,
                "{group_option} must precede the subcommand"
            );
        }
        for flag in &flags {
            let pos = args.iter().position(|a| a == flag).unwrap();
            assert!(pos > subcommand, "{flag} must follow the subcommand");
        }
    }

    #[test]
    fn engine_args_work_without_flags() {
        let args = build_engine_args("prepare", "in.txt", "/cfg.yaml", &[]);
        assert_eq!(
            args,
            vec!["--config", "/cfg.yaml", "--json-events", "prepare", "in.txt"]
        );
    }
}
