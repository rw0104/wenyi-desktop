//! Wenyi Desktop — Rust command layer.
//!
//! Responsibilities:
//! - spawn the `wenyi-core` sidecar (the PyInstaller-packaged `trans-novel` engine),
//! - relay the sidecar's JSONL event stream to the webview frontend,
//! - inject API keys / proxy / config path as environment variables,
//! - expose file dialogs and "open output" helpers.
//!
//! The translation engine remains a separate process so long-running work is
//! crash-isolated, interruptible, and resumable — matching wenyi's own file-lock
//! and checkpoint design.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// Configured runtime for the sidecar process.
struct SidecarState(Mutex<Option<CommandChild>>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslateOptions {
    /// Absolute path to the input book / subtitle.
    input: String,
    /// Optional config.yaml path to use (defaults to app data dir).
    config: Option<String>,
    /// Extra CLI flags forwarded verbatim (e.g. ["--bilingual"]).
    flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest {
    command: String, // translate | prepare | review | status | assemble
    input: String,
    config: Option<String>,
    flags: Vec<String>,
    /// API keys to inject per provider; key names are env var names.
    env: std::collections::HashMap<String, String>,
    /// Optional proxy, e.g. "http://127.0.0.1:10808".
    proxy: Option<String>,
}

/// Kill a running sidecar (cancel an in-flight translation).
#[tauri::command]
async fn cancel(state: State<'_, SidecarState>) -> Result<(), String> {
    if let Some(child) = state.0.lock().unwrap().take() {
        let _ = child.kill();
    }
    Ok(())
}

/// Launch the translation engine and stream its JSONL events to the frontend.
///
/// The sidecar must have been built by `sidecar/build_sidecar.ps1` and placed at
/// `src-tauri/binaries/wenyi-core[-<target-triple>][.exe]`. Tauri resolves the
/// correct binary for the host automatically via `app.shell().sidecar("wenyi-core")`.
#[tauri::command]
async fn run_engine(
    app: tauri::AppHandle,
    state: State<'_, SidecarState>,
    request: RunRequest,
) -> Result<(), String> {
    // Cancel any previous run.
    if let Some(child) = state.0.lock().unwrap().take() {
        let _ = child.kill();
    }

    let sidecar = app
        .shell()
        .sidecar("wenyi-core")
        .map_err(|e| e.to_string())?;

    // Assemble args: <command> <input> [--config PATH] [flags...] --json-events
    let mut args: Vec<String> = vec![request.command.clone(), request.input.clone()];
    if let Some(cfg) = request.config.as_ref() {
        args.push("--config".into());
        args.push(cfg.clone());
    }
    for flag in request.flags.iter() {
        args.push(flag.clone());
    }
    // The engine must emit machine-readable JSONL (P0 addition to trans_novel).
    args.push("--json-events".into());

    let (mut rx, child) = sidecar
        .args(args)
        .envs(request.env.iter().map(|(k, v)| (k.clone(), v.clone())))
        .envs({
            let mut proxy_env = std::collections::HashMap::new();
            if let Some(p) = request.proxy.as_ref() {
                proxy_env.insert("HTTP_PROXY".to_string(), p.clone());
                proxy_env.insert("HTTPS_PROXY".to_string(), p.clone());
                proxy_env.insert("ALL_PROXY".to_string(), p.clone());
            }
            proxy_env
        })
        .spawn()
        .map_err(|e| e.to_string())?;

    *state.0.lock().unwrap() = Some(child);

    // Stream events to the frontend. We parse line-delimited JSON and re-emit
    // each object under the "engine-event" Tauri event.
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(line) => {
                let text = String::from_utf8_lossy(&line);
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
                            // Non-JSON line: forward as a log message.
                            let _ = app.emit(
                                "engine-event",
                                serde_json::json!({"event": "log", "message": raw}),
                            );
                        }
                    }
                }
            }
            CommandEvent::Stderr(line) => {
                let text = String::from_utf8_lossy(&line);
                let _ = app.emit(
                    "engine-event",
                    serde_json::json!({"event": "stderr", "message": text}),
                );
            }
            CommandEvent::Terminated(payload) => {
                let _ = app.emit(
                    "engine-event",
                    serde_json::json!({
                        "event": "terminated",
                        "exit_code": payload.code
                    }),
                );
                break;
            }
            _ => {}
        }
    }

    // Clear the child handle once finished.
    *state.0.lock().unwrap() = None;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SidecarState(Mutex::new(None)))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![run_engine, cancel])
        .run(tauri::generate_context!())
        .expect("error while running Wenyi Desktop");
}