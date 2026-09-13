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
    /// Model id for the built-in providers. Empty keeps the engine preset's model, which
    /// pins a single older id for every tier; set this to use a newer one.
    pub model_override: String,
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
            model_override: String::new(),
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

/// Storage name used for a custom endpoint's key when the user has not chosen one.
pub const DEFAULT_CUSTOM_KEY_ENV: &str = "CUSTOM_API_KEY";

impl Settings {
    /// Credential name (an environment-variable name) under which this configuration's API
    /// key is stored and handed to the engine.
    ///
    /// The name is an advanced detail, so it is defaulted rather than left blank: asking a
    /// user to invent a variable name before they have anywhere to paste the key is an
    /// indirection that leaves them unable to supply credentials at all.
    pub fn api_key_env(&self) -> String {
        match self.provider.as_str() {
            "gemini" => "GEMINI_API_KEY".into(),
            "custom" => {
                let named = self.custom_key_env.trim();
                if named.is_empty() {
                    DEFAULT_CUSTOM_KEY_ENV.into()
                } else {
                    named.into()
                }
            }
            _ => "DEEPSEEK_API_KEY".into(),
        }
    }

    /// Whether an API key is mandatory.
    ///
    /// A custom endpoint is frequently a local model that needs no credentials, so its key
    /// is optional; the hosted providers require one.
    pub fn requires_api_key(&self) -> bool {
        self.provider != "custom"
    }

    /// The model id this configuration will actually request.
    ///
    /// One field serves every provider: for the built-in presets it replaces the pinned
    /// model, and for a custom endpoint it is the model id. `custom_model` is still read as
    /// a fallback so settings written by an earlier build keep working.
    pub fn model_id(&self) -> String {
        let override_model = self.model_override.trim();
        if !override_model.is_empty() {
            return override_model.to_string();
        }
        if self.provider == "custom" {
            return self.custom_model.trim().to_string();
        }
        String::new()
    }

    /// Reject a configuration that would make the engine exit before emitting any event.
    ///
    /// A configuration-level failure produces no JSONL at all, so the shell can only report
    /// a generic "engine stopped" message. Catching these here turns each one into a
    /// specific instruction instead.
    pub fn validate_for_run(&self, key_stored: bool) -> Result<(), String> {
        if self.provider == "custom" {
            if self.custom_base_url.trim().is_empty() {
                return Err("请填写「接口地址 base_url」。".into());
            }
            if self.model_id().is_empty() {
                return Err("请填写「模型 ID」——可以点「获取模型列表」从接口拉取。".into());
            }
            // A key is optional (local models), but the endpoint is remote and unauthenticated
            // requests are the most common reason a run fails at the first call.
            if !key_stored {
                return Err(format!(
                    "尚未存入密钥。如果 {} 需要鉴权，请把密钥填入「接口密钥」并点「存入凭据库」\
                     （将作为 {} 发送）；本地模型可忽略此提示。",
                    self.custom_base_url.trim(),
                    self.api_key_env()
                ));
            }
        } else if self.requires_api_key() && !key_stored {
            return Err(format!(
                "尚未存入 {}。请在「接口密钥」填入并点「存入凭据库」。",
                self.api_key_env()
            ));
        }
        Ok(())
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
    let key_stored = crate::secrets::get(&settings.api_key_env()).is_some();
    write_engine_config(app, settings, key_stored)
}

/// Write `config.yaml` for the engine, derived entirely from `settings`.
pub fn write_engine_config(
    app: &AppHandle,
    settings: &Settings,
    custom_key_stored: bool,
) -> Result<(), String> {
    let path = config_file(app)?;
    fs::write(&path, render_config_yaml(settings, custom_key_stored)).map_err(|e| e.to_string())
}

/// Quote a scalar for YAML so URLs and model IDs cannot break the document.
fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build the engine configuration document.
///
/// Only documented top-level sections are emitted: the engine rejects unknown keys, so an
/// accidental field here would make every run fail at startup.
///
/// `custom_key_stored` gates `api_key_env` for a custom endpoint: declaring a credential
/// variable that holds nothing would make a local, keyless endpoint look misconfigured.
pub fn render_config_yaml(settings: &Settings, custom_key_stored: bool) -> String {
    let override_model = settings.model_override.trim();
    // The preset pins one model id across all three tiers. Replacing those profiles keeps
    // the preset's connection (endpoint and key variable) while choosing the model.
    let preset_override = |preset: &str| {
        let mut out = format!("llm:\n  preset: {preset}\n  models:\n");
        for profile in ["default_strong", "default_cheap", "default_fast"] {
            out.push_str(&format!(
                "    {profile}:\n      provider: default\n      model: {}\n",
                yaml_scalar(override_model)
            ));
        }
        out
    };

    let llm = match settings.provider.as_str() {
        "gemini" => {
            if override_model.is_empty() {
                "llm:\n  preset: gemini\n".to_string()
            } else {
                preset_override("gemini")
            }
        }
        "custom" => {
            let key_line = if custom_key_stored {
                format!(
                    "      api_key_env: {}\n",
                    yaml_scalar(&settings.api_key_env())
                )
            } else {
                String::new()
            };
            format!(
                "llm:\n  providers:\n    custom:\n      kind: openai-compatible\n      base_url: {}\n{key_line}  models:\n    custom_model:\n      provider: custom\n      model: {}\n  tiers:\n    strong: custom_model\n    cheap: custom_model\n    fast: custom_model\n",
                yaml_scalar(settings.custom_base_url.trim()),
                yaml_scalar(&settings.model_id()),
            )
        }
        _ => {
            if override_model.is_empty() {
                "llm:\n  preset: deepseek\n".to_string()
            } else {
                preset_override("deepseek")
            }
        }
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

/// How the current settings will actually be used, for display in the UI.
///
/// This exists because the provider selector decides everything: values typed into the
/// custom endpoint fields are stored but ignored unless the provider is set to `custom`.
/// Showing the resolved target, and calling out ignored input, is what stops a user from
/// believing their endpoint is in use when it is not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfig {
    pub endpoint: String,
    pub model: String,
    pub provider_kind: String,
    pub api_key_env: String,
    /// True when the custom fields hold values that this configuration will not use.
    pub custom_fields_ignored: bool,
    /// Human-readable warnings worth surfacing next to the setting.
    pub notes: Vec<String>,
}

pub fn describe(settings: &Settings, custom_key_stored: bool) -> EffectiveConfig {
    let custom_filled = !settings.custom_base_url.trim().is_empty()
        || !settings.custom_model.trim().is_empty();
    let mut notes = Vec::new();

    let (endpoint, model, provider_kind) = match settings.provider.as_str() {
        "gemini" => (
            "generativelanguage.googleapis.com".to_string(),
            if settings.model_override.trim().is_empty() {
                "gemini-3.6-flash (预设默认)".to_string()
            } else {
                settings.model_override.trim().to_string()
            },
            "gemini".to_string(),
        ),
        "custom" => {
            let base = settings.custom_base_url.trim();
            let mdl = settings.model_id();
            if base.is_empty() {
                notes.push("自定义提供方还需要填写 base_url。".into());
            }
            if mdl.is_empty() {
                notes.push("自定义提供方还需要填写模型 ID。".into());
            }
            (
                if base.is_empty() { "(未填写)".into() } else { base.to_string() },
                if mdl.is_empty() { "(未填写)".into() } else { mdl },
                "openai-compatible".to_string(),
            )
        }
        _ => (
            "https://api.deepseek.com".to_string(),
            if settings.model_override.trim().is_empty() {
                "deepseek-flash (预设默认，较旧)".to_string()
            } else {
                settings.model_override.trim().to_string()
            },
            "deepseek".to_string(),
        ),
    };

    // Most actionable first: an ignored field or a missing key is a configuration mistake,
    // whereas the pinned-model note is advice.
    if custom_filled && settings.provider != "custom" {
        notes.push(
            "自定义接口地址和模型 ID 已填写，但上面的提供方不是「自定义」，因此它们会被忽略。\
             要使用它们，请把提供方切换为「自定义」。"
                .into(),
        );
    }

    if settings.provider == "custom" && !custom_key_stored {
        let named = !settings.custom_key_env.trim().is_empty();
        notes.push(
            if named {
                format!(
                    "尚未存入密钥：如果这个接口需要鉴权，请把密钥存入凭据库（将作为 {} 发送）。\
                     本地模型不需要密钥。",
                    settings.api_key_env()
                )
            } else {
                "尚未存入密钥：如果这个接口需要鉴权（中转站、云服务），请把密钥填入\
                 「接口密钥」并存入凭据库。本地模型不需要密钥。"
                    .into()
            },
        );
    }

    if settings.provider != "custom" && settings.model_id().is_empty() {
        notes.push(
            "当前使用引擎预设里写死的模型。如果这个模型已过时，请在上面填写模型 ID，\
             或点「获取模型列表」从接口拉取可用模型。"
                .into(),
        );
    }

    EffectiveConfig {
        endpoint,
        model,
        provider_kind,
        api_key_env: settings.api_key_env(),
        custom_fields_ignored: custom_filled && settings.provider != "custom",
        notes,
    }
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

        // A custom endpoint always has a usable credential name, so a user has somewhere
        // to paste a key without first inventing a variable name. The key stays optional
        // because the endpoint may be a local model that needs no credentials.
        s.provider = "custom".into();
        s.custom_key_env = String::new();
        assert_eq!(s.api_key_env(), "CUSTOM_API_KEY");
        assert!(!s.requires_api_key());

        s.custom_key_env = "  MY_API_KEY  ".into();
        assert_eq!(s.api_key_env(), "MY_API_KEY");
        assert!(!s.requires_api_key());

        // Hosted providers still require a key.
        s.provider = "deepseek".into();
        assert!(s.requires_api_key());
    }

    #[test]
    fn deepseek_config_uses_the_preset_and_respects_switches() {
        let mut s = Settings::default();
        s.review = false;
        s.polish = false;
        s.bilingual = true;
        let yaml = render_config_yaml(&s, false);

        assert!(yaml.contains("preset: deepseek"));
        assert!(yaml.contains("review: false"));
        assert!(yaml.contains("polish: false"));
        assert!(yaml.contains("bilingual: true"));
        assert!(yaml.contains("source: \"auto\""));
        assert!(yaml.contains("target: \"zh\""));
        assert!(!yaml.contains("api_key_env"));
    }

    #[test]
    fn custom_provider_emits_a_complete_route() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "http://localhost:11434/v1".into();
        s.custom_model = "qwen2.5:7b".into();
        s.custom_key_env = "MY_KEY".into();
        let yaml = render_config_yaml(&s, true);

        assert!(yaml.contains("kind: openai-compatible"));
        assert!(yaml.contains("base_url: \"http://localhost:11434/v1\""));
        assert!(yaml.contains("model: \"qwen2.5:7b\""));
        assert!(yaml.contains("api_key_env: \"MY_KEY\""));
        // Without a preset every tier must be mapped explicitly.
        for tier in ["strong", "cheap", "fast"] {
            assert!(yaml.contains(&format!("{tier}: custom_model")), "missing {tier}");
        }
    }

    /// A local, keyless endpoint must not be told to read a credential variable, or the
    /// engine looks for a key that legitimately does not exist.
    #[test]
    fn custom_provider_without_a_stored_key_omits_the_variable() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "http://localhost:8000/v1".into();
        s.custom_model = "local".into();
        s.custom_key_env = String::new();

        let yaml = render_config_yaml(&s, false);
        assert!(!yaml.contains("api_key_env"));
        assert!(yaml.contains("kind: openai-compatible"));

        // Once a key is stored, the variable is declared using the defaulted name.
        let yaml_with_key = render_config_yaml(&s, true);
        assert!(yaml_with_key.contains("api_key_env: \"CUSTOM_API_KEY\""));
    }

    /// The engine rejects unknown top-level sections, so a stray key would fail every run.
    #[test]
    fn only_documented_top_level_sections_are_emitted() {
        let yaml = render_config_yaml(&Settings::default(), false);
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
        let yaml = render_config_yaml(&s, false);
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

    /// One model field serves both cases, and the legacy custom field is still honoured.
    #[test]
    fn model_id_resolves_for_every_provider() {
        let mut s = Settings::default();
        // No override: a preset has no explicit model, which is what marks it as defaulted.
        assert_eq!(s.model_id(), "");

        s.model_override = "deepseek-chat".into();
        assert_eq!(s.model_id(), "deepseek-chat");

        // Custom falls back to its own field so settings from an earlier build still work.
        s.provider = "custom".into();
        s.model_override = String::new();
        s.custom_model = "legacy-model".into();
        assert_eq!(s.model_id(), "legacy-model");

        // The shared field wins when both are present.
        s.model_override = "new-model".into();
        assert_eq!(s.model_id(), "new-model");
    }

    /// A custom endpoint that would produce `model: ""` makes the engine abort during
    /// config validation with no JSONL at all, which the shell can only report vaguely.
    #[test]
    fn a_custom_endpoint_without_a_model_is_rejected_before_running() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "https://relay.example/v1".into();
        s.custom_key_env = "RELAY_KEY".into();

        let error = s.validate_for_run(true).unwrap_err();
        assert!(error.contains("模型 ID"), "got {error}");

        s.model_override = "some-model".into();
        assert!(s.validate_for_run(true).is_ok());
    }

    #[test]
    fn a_custom_endpoint_without_a_base_url_is_rejected() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.model_override = "some-model".into();
        let error = s.validate_for_run(true).unwrap_err();
        assert!(error.contains("base_url"), "got {error}");
    }

    /// The key is optional for a custom endpoint (local models), but running a remote
    /// endpoint with nothing stored is the most common failure and is named explicitly.
    #[test]
    fn an_unsaved_key_is_reported_with_the_variable_name() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "https://relay.example/v1".into();
        s.model_override = "some-model".into();
        s.custom_key_env = "RELAY_KEY".into();

        let error = s.validate_for_run(false).unwrap_err();
        assert!(error.contains("RELAY_KEY"), "got {error}");

        // Hosted providers still require a key.
        let mut hosted = Settings::default();
        let hosted_error = hosted.validate_for_run(false).unwrap_err();
        assert!(hosted_error.contains("DEEPSEEK_API_KEY"), "got {hosted_error}");
        assert!(hosted.validate_for_run(true).is_ok());
    }
    #[test]
    fn deepseek_describe_reports_the_official_endpoint() {
        let eff = describe(&Settings::default(), false);
        assert_eq!(eff.endpoint, "https://api.deepseek.com");
        assert!(eff.model.starts_with("deepseek-flash"), "got {}", eff.model);
        assert_eq!(eff.api_key_env, "DEEPSEEK_API_KEY");
        assert!(!eff.custom_fields_ignored);
        // No override set, so the preset's pinned model is flagged as a default.
        assert_eq!(eff.notes.len(), 1);
        assert!(eff.notes[0].contains("预设"), "got {:?}", eff.notes);
    }

    /// The presets pin one model id for every tier; a user must be able to replace it.
    #[test]
    fn model_override_is_reported_and_replaces_every_tier() {
        let mut s = Settings::default();
        s.model_override = "deepseek-chat".into();
        let eff = describe(&s, false);
        assert_eq!(eff.model, "deepseek-chat");
        assert!(eff.notes.is_empty(), "got {:?}", eff.notes);

        let yaml = render_config_yaml(&s, false);
        assert!(yaml.contains("preset: deepseek"));
        assert!(yaml.contains("model: \"deepseek-chat\""));
        for profile in ["default_strong", "default_cheap", "default_fast"] {
            assert!(yaml.contains(profile), "missing {profile}");
        }
        // The preset's older id must not survive anywhere in the document.
        assert!(!yaml.contains("deepseek-flash"));
    }

    #[test]
    fn model_override_applies_to_gemini_too() {
        let mut s = Settings::default();
        s.provider = "gemini".into();
        s.model_override = "gemini-3.6-pro".into();
        let yaml = render_config_yaml(&s, false);
        assert!(yaml.contains("preset: gemini"));
        assert!(yaml.contains("model: \"gemini-3.6-pro\""));
        assert!(!yaml.contains("gemini-3.6-flash"));
    }

    #[test]
    fn no_override_keeps_the_plain_preset() {
        let yaml = render_config_yaml(&Settings::default(), false);
        assert_eq!(yaml.matches("preset: deepseek").count(), 1);
        assert!(!yaml.contains("default_strong"));
    }

    /// The exact situation that made a user believe their relay was in use when the app
    /// was calling the official endpoint with a relay key.
    #[test]
    fn custom_fields_filled_while_provider_is_not_custom_are_reported_as_ignored() {
        let mut s = Settings::default();
        s.custom_base_url = "https://relay.example/v1".into();
        s.custom_model = "some-model".into();
        // provider stays "deepseek"
        let eff = describe(&s, false);

        assert!(eff.custom_fields_ignored);
        assert_eq!(eff.endpoint, "https://api.deepseek.com");
        assert!(eff.model.starts_with("deepseek-flash"), "got {}", eff.model);
        // Two notes: the ignored custom fields, and the pinned preset model.
        assert_eq!(eff.notes.len(), 2);
        assert!(eff.notes[0].contains("忽略"), "got {:?}", eff.notes);
    }

    #[test]
    fn selecting_the_custom_provider_uses_the_typed_values() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "https://relay.example/v1".into();
        s.custom_model = "some-model".into();
        s.custom_key_env = "RELAY_KEY".into();
        // With a key stored there is nothing left to warn about.
        let eff = describe(&s, true);

        assert!(!eff.custom_fields_ignored);
        assert_eq!(eff.endpoint, "https://relay.example/v1");
        assert_eq!(eff.model, "some-model");
        assert_eq!(eff.api_key_env, "RELAY_KEY");
        assert!(eff.notes.is_empty(), "got {:?}", eff.notes);
    }

    /// A relay or cloud endpoint needs a key, and leaving one unsaved is the most common
    /// way a custom configuration fails, so it is called out before the run is attempted.
    #[test]
    fn custom_provider_without_a_stored_key_is_called_out() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        s.custom_base_url = "https://relay.example/v1".into();
        s.custom_model = "some-model".into();

        // No variable name chosen: the note points at the key field, which is what the
        // user can actually act on. Naming a variable they never picked would be noise.
        let eff = describe(&s, false);
        assert_eq!(eff.notes.len(), 1);
        assert!(eff.notes[0].contains("尚未存入密钥"), "got {:?}", eff.notes);
        assert_eq!(eff.api_key_env, DEFAULT_CUSTOM_KEY_ENV);

        // With an explicit name, the note states what the key will be sent as.
        s.custom_key_env = "RELAY_KEY".into();
        let named = describe(&s, false);
        assert!(named.notes[0].contains("RELAY_KEY"), "got {:?}", named.notes);
    }

    #[test]
    fn custom_provider_missing_fields_is_called_out() {
        let mut s = Settings::default();
        s.provider = "custom".into();
        let eff = describe(&s, false);
        assert_eq!(eff.endpoint, "(未填写)");
        assert_eq!(eff.model, "(未填写)");
        // Two missing-field notes plus the unsaved-key note.
        assert_eq!(eff.notes.len(), 3);
    }

    #[test]
    fn gemini_describe_uses_its_own_key_variable() {
        let mut s = Settings::default();
        s.provider = "gemini".into();
        let eff = describe(&s, false);
        assert_eq!(eff.api_key_env, "GEMINI_API_KEY");
        assert_eq!(eff.provider_kind, "gemini");
    }
}
