//! Persistent desktop settings and engine `config.yaml` generation.
//!
//! Settings live in the app data directory, never in the engine's own files, so that
//! regenerating `config.yaml` is always safe and reproducible. API keys are *not* stored
//! here; they live in the OS credential store (see `secrets.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const SETTINGS_FILE: &str = "desktop-settings.json";
pub const HISTORY_FILE: &str = "history.json";

/// Disk locations used by the shell. `workspace_dir` is the engine's working directory,
/// which is where its relative `state/` tree is created.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paths {
    pub config_dir: String,
    pub workspace_dir: String,
    pub config_file: String,
    pub state_dir: String,
    pub settings_file: String,
    pub history_file: String,
}

/// User-facing settings persisted between launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `auto` or an ISO 639-1 code.
    pub source_lang: String,
    pub target_lang: String,
    /// `deepseek` | `gemini` | `custom` (any OpenAI-compatible endpoint).
    pub provider: String,
    /// Custom provider fields, used only when `provider == "custom"`.
    pub custom_base_url: String,
    pub custom_model: String,
    /// Environment variable name holding the custom endpoint's key; empty means none.
    pub custom_key_env: String,
    /// Optional HTTP proxy applied to the engine process, e.g. http://127.0.0.1:10808
    pub proxy: String,
    pub polish: bool,
    pub review: bool,
    pub bilingual: bool,
    pub mono: bool,
    pub book_understanding: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source_lang: "auto".into(),
            target_lang: "zh".into(),
            provider: "deepseek".into(),
            custom_base_url: String::new(),
            custom_model: String::new(),
            custom_key_env: String::new(),
            proxy: String::new(),
            polish: true,
            review: true,
            bilingual: false,
            mono: true,
            book_understanding: true,
        }
    }
}

impl Settings {
    /// Environment variable that holds the API key for the selected provider.
    pub fn api_key_env(&self) -> String {
        match self.provider.as_str() {
            "gemini" => "GEMINI_API_KEY".into(),
            "custom" if !self.custom_key_env.trim().is_empty() => self.custom_key_env.trim().into(),
            "custom" => String::new(),
            _ => "DEEPSEEK_API_KEY".into(),
        }
    }

    /// Whether the provider needs an API key at all (local endpoints do not).
    pub fn requires_api_key(&self) -> bool {
        !self.api_key_env().is_empty()
    }
}

/// Return the app data directory, creating it when missing.
pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Engine working directory: the sidecar runs here so `state/` lands in one known place.
pub fn workspace_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = config_dir(app)?.join("workspace");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(SETTINGS_FILE))
}

pub fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(HISTORY_FILE))
}

pub fn config_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(workspace_dir(app)?.join("config.yaml"))
}

/// Resolve every path the frontend needs in one call.
pub fn paths(app: &AppHandle) -> Result<Paths, String> {
    let config = config_dir(app)?;
    let workspace = workspace_dir(app)?;
    Ok(Paths {
        config_file: workspace.join("config.yaml").to_string_lossy().into_owned(),
        state_dir: workspace.join("state").to_string_lossy().into_owned(),
        settings_file: config.join(SETTINGS_FILE).to_string_lossy().into_owned(),
        history_file: config.join(HISTORY_FILE).to_string_lossy().into_owned(),
        config_dir: config.to_string_lossy().into_owned(),
        workspace_dir: workspace.to_string_lossy().into_owned(),
    })
}

/// Load settings, falling back to defaults when absent or unreadable.
///
/// A corrupt settings file must not brick the app; defaults are returned instead and the
/// file is rewritten on the next save.
pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.is_file() {
        return Ok(Settings::default());
    }
    match fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(Settings::default()),
    }
}

/// Persist settings and regenerate the engine configuration from them.
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())?;
    write_engine_config(app, settings)
}

/// Write `config.yaml` for the engine, derived entirely from `settings`.
pub fn write_engine_config(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = config_file(app)?;
    fs::write(&path, render_config_yaml(settings)).map_err(|e| e.to_string())
}

/// Quote a scalar for YAML so URLs and model IDs cannot break the document.
fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build the engine configuration document.
///
/// Only documented top-level sections are emitted: the engine rejects unknown keys, so an
/// accidental field here would make every run fail at startup.
pub fn render_config_yaml(settings: &Settings) -> String {
    let llm = match settings.provider.as_str() {
        "gemini" => "llm:\n  preset: gemini\n".to_string(),
        "custom" => {
            let key_line = if settings.custom_key_env.trim().is_empty() {
                String::new()
            } else {
                format!("      api_key_env: {}\n", yaml_scalar(settings.custom_key_env.trim()))
            };
            format!(
                "llm:\n  providers:\n    custom:\n      kind: openai-compatible\n      base_url: {}\n{key_line}  models:\n    custom_model:\n      provider: custom\n      model: {}\n  tiers:\n    strong: custom_model\n    cheap: custom_model\n    fast: custom_model\n",
                yaml_scalar(settings.custom_base_url.trim()),
                yaml_scalar(settings.custom_model.trim()),
            )
        }
        _ => "llm:\n  preset: deepseek\n".to_string(),
    };

    format!(
        "# Generated by Wenyi Desktop from desktop-settings.json.\n\
         # Manual edits are overwritten the next time settings are saved.\n\
         \n\
         language:\n  source: {source}\n  target: {target}\n\
         \n\
         {llm}\
         \n\
         pipeline:\n  review: {review}\n  polish: {polish}\n  book_understanding: {understanding}\n\
         \n\
         output:\n  mono: {mono}\n  bilingual: {bilingual}\n  bilingual_order: target_first\n  about_page: true\n",
        source = yaml_scalar(&settings.source_lang),
        target = yaml_scalar(&settings.target_lang),
        review = settings.review,
        polish = settings.polish,
        understanding = settings.book_understanding,
        mono = settings.mono,
        bilingual = settings.bilingual,
    )
}

/// Append or refresh one entry in the run history so interrupted work can be resumed.
pub fn remember_run(app: &AppHandle, input: &str, command: &str, at: &str) -> Result<(), String> {
    let path = history_path(app)?;
    let mut history = read_history(&path);
    history.retain(|row| row.input != input);
    history.insert(
        0,
        HistoryEntry {
            input: input.to_string(),
            command: command.to_string(),
            updated_at: at.to_string(),
        },
    );
    history.truncate(50);
    let text = serde_json::to_string_pretty(&history).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

pub fn read_history(path: &Path) -> Vec<HistoryEntry> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub input: String,
    pub command: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_env_follows_the_provider() {
        let mut s = Settings::default();
        assert_eq!(s.api_key_env(), "DEEPSEEK_API_KEY");

        s.provider = "gemini".into();
        assert_eq!(s.api_key_env(), "GEMINI_API_KEY");

        // A custom endpoint without a named variable needs no key (local models).
        s.provider = "custom".into();
        s.custom_key_env = String::new();
        assert_eq!(s.api_key_env(), "");
        assert!(!s.requires_api_key());

        s.custom_key_env = "  MY_API_KEY  ".into();
        assert_eq!(s.api_key_env(), "MY_API_KEY");
        assert!(s.requires_api_key());
    }

    #[test]
    fn deepseek_config_uses_the_preset_and_respects_switches() {
        let mut s = Settings::default();
        s.review = false;
        s.polish = false;
        s.bilingual = true;
        let yaml = render_config_yaml(&s);

        assert!(yaml.contains("preset: deepseek"));
        assert!(yaml.contains("review: false"));
        assert!(yaml.contains("polish: false"));
        assert!(yaml.contains("bilingual: true"));
        assert!(yaml.contains("source: \"auto\""));
        assert!(yaml.contains("target: \"zh\""));
    }

    #[test]
    fn custom_provider_emits_a_complete_route() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "http://localhost:11434/v1".into();
        s.custom_model = "qwen2.5:7b".into();
        s.custom_key_env = "MY_KEY".into();
        let yaml = render_config_yaml(&s);

        assert!(yaml.contains("kind: openai-compatible"));
        assert!(yaml.contains("base_url: \"http://localhost:11434/v1\""));
        assert!(yaml.contains("model: \"qwen2.5:7b\""));
        assert!(yaml.contains("api_key_env: \"MY_KEY\""));
        // Without a preset every tier must be mapped explicitly.
        for tier in ["strong", "cheap", "fast"] {
            assert!(yaml.contains(&format!("{tier}: custom_model")), "missing {tier}");
        }
    }

    #[test]
    fn custom_provider_without_a_key_variable_omits_the_field() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "http://localhost:8000/v1".into();
        s.custom_model = "local".into();
        s.custom_key_env = String::new();

        let yaml = render_config_yaml(&s);
        assert!(!yaml.contains("api_key_env"));
        assert!(yaml.contains("kind: openai-compatible"));
    }

    /// The engine rejects unknown top-level sections, so a stray key would fail every run.
    #[test]
    fn only_documented_top_level_sections_are_emitted() {
        let yaml = render_config_yaml(&Settings::default());
        for line in yaml.lines() {
            if line.is_empty() || line.starts_with('#') || line.starts_with(' ') {
                continue;
            }
            let section = line.trim_end_matches(':');
            assert!(
                ["language", "llm", "pipeline", "output"].contains(&section),
                "unexpected top-level section: {line}"
            );
        }
    }

    #[test]
    fn yaml_scalars_are_escaped() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_model = "weird\"model\\name".into();
        s.custom_base_url = "http://x/v1".into();
        let yaml = render_config_yaml(&s);
        assert!(yaml.contains(r#"model: "weird\"model\\name""#));
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        // `load` needs an AppHandle, so assert the serde contract it relies on instead.
        let parsed: Result<Settings, _> = serde_json::from_str("{not json");
        assert!(parsed.is_err());
        let fallback = parsed.unwrap_or_default();
        assert_eq!(fallback.target_lang, "zh");
        assert_eq!(fallback.provider, "deepseek");
    }
}
