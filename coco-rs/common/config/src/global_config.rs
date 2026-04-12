use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Per-user global config. Separate from Settings.
/// TS: GlobalConfig type in utils/config.ts, stored at ~/.claude.json
/// Rust: stored at ~/.coco.json (or COCO_CONFIG_DIR/.coco.json)
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
/// Respects COCO_CONFIG_DIR env var, defaults to ~/.coco
pub fn config_home() -> PathBuf {
    if let Ok(dir) = std::env::var("COCO_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs_home().join(".coco")
}

/// Get the global config file path (~/.coco.json).
pub fn global_config_path() -> PathBuf {
    dirs_home().join(".coco.json")
}

fn dirs_home() -> PathBuf {
    // Simple home dir detection
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
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
