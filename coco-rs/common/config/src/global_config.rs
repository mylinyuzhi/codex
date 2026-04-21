use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Per-user global config. Separate from Settings.
/// TS: GlobalConfig type in utils/config.ts, stored at ~/.claude.json
/// Rust: stored at ~/.coco.json (or $COCO_CONFIG_DIR/global.json when set)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub projects: HashMap<String, ProjectConfig>,
    pub session_costs: HashMap<String, SessionCostState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub companion: Option<CompanionConfig>,
    /// Has the user completed onboarding?
    #[serde(default)]
    pub has_completed_onboarding: bool,
    /// Cached org-level settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_settings_cache: Option<serde_json::Value>,
}

/// Per-project config within GlobalConfig.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub costs: Option<SessionCostState>,
}

/// Session cost tracking state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionCostState {
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

/// Companion pet config (buddy).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CompanionConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Get the config home directory.
///
/// Respects `COCO_CONFIG_DIR` env var, defaults to `~/.coco`. Delegates to
/// `coco_utils_common::find_coco_home` so MCP and every other consumer
/// agree on one implementation (empty-string filtering, cross-platform
/// `dirs::home_dir()` fallback, last-resort cwd fallback).
pub fn config_home() -> PathBuf {
    coco_utils_common::find_coco_home()
}

/// Get the global config file path.
///
/// Priority:
/// 1. If `COCO_CONFIG_DIR` is set, put `global.json` inside that dir so
///    the whole coco workspace (settings + global state + sessions)
///    moves as a unit. Useful for sandboxed / per-project setups.
/// 2. Otherwise, fall back to `~/.coco.json` — TS parity with
///    `~/.claude.json`, a sibling of `~/.coco/`.
pub fn global_config_path() -> PathBuf {
    if let Some(custom) =
        std::env::var_os(coco_utils_common::COCO_CONFIG_DIR_ENV).filter(|s| !s.is_empty())
    {
        return PathBuf::from(custom).join("global.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".coco.json")
}

/// Load global config from disk.
pub fn load_global_config() -> anyhow::Result<GlobalConfig> {
    let path = global_config_path();
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let contents = std::fs::read_to_string(&path)?;
    let config: GlobalConfig = serde_json::from_str(&contents)?;
    Ok(config)
}

/// Write global config to disk.
pub fn write_global_config(config: &GlobalConfig) -> anyhow::Result<()> {
    let path = global_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, contents)?;
    Ok(())
}

/// Get the user settings path.
pub fn user_settings_path() -> PathBuf {
    config_home().join("settings.json")
}

/// Get the project settings path.
pub fn project_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.json")
}

/// Get the local (gitignored) settings path.
pub fn local_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.local.json")
}

/// Get the managed settings path (enterprise/MDM).
pub fn managed_settings_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/CoCo/managed-settings.json")
    }
    #[cfg(target_os = "linux")]
    {
        PathBuf::from("/etc/coco/managed-settings.json")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
    {
        PathBuf::from(r"C:\Program Files\CoCo\managed-settings.json")
    }
}
