pub mod merge;
pub mod policy;
pub mod source;
pub mod validation;
pub mod watcher;

use coco_types::PermissionMode;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

pub use source::SettingSource;

/// The merged settings snapshot. Immutable after loading.
/// TS: SettingsJson type in types.ts (Zod schema)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // === Auth ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_helper: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_login_method: Option<String>,

    // === Permissions ===
    pub permissions: PermissionsConfig,

    // === Model ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_overrides: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_thinking_enabled: Option<bool>,

    // === Environment ===
    #[serde(default)]
    pub env: HashMap<String, String>,

    // === Hooks ===
    /// Deserialized by coco-hooks, kept as Value here (avoids L1→L4 dep).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    #[serde(default)]
    pub disable_all_hooks: bool,

    // === MCP ===
    #[serde(default)]
    pub allowed_mcp_servers: Vec<AllowedMcpServerEntry>,
    #[serde(default)]
    pub denied_mcp_servers: Vec<DeniedMcpServerEntry>,
    #[serde(default)]
    pub enable_all_project_mcp_servers: bool,

    // === Shell ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_shell: Option<String>,

    // === Display ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub syntax_highlighting_disabled: bool,

    // === Plugins ===
    #[serde(default)]
    pub enabled_plugins: HashMap<String, PluginConfig>,

    // === Worktree ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeConfig>,

    // === Plans ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plans_directory: Option<String>,

    // === Auto-Mode ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_mode: Option<AutoModeConfig>,

    // === Policy ===
    /// When true, only managed (policy-level) hooks are allowed to run.
    #[serde(default)]
    pub allow_managed_hooks_only: bool,
    /// When true, only plugin-level customization is permitted.
    #[serde(default)]
    pub strict_plugin_only_customization: bool,

    // === File Checkpointing ===
    /// When false, disables file checkpointing for rewind.
    /// TS: `fileCheckpointingEnabled` in supportedSettings.ts
    #[serde(default = "default_true")]
    pub file_checkpointing_enabled: bool,

    // === Attribution ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_co_authored_by: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_git_instructions: Option<bool>,
}

/// Permission rules configuration within settings.
///
/// Rules are stored on disk as string arrays matching the TS format:
/// `{ "permissions": { "allow": ["Bash", "Bash(git *)"], "deny": [...] } }`
///
/// Use `coco_permissions::parse_rule_string()` to convert these strings
/// into typed `PermissionRule` values at load time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_mode: Option<PermissionMode>,
    pub disable_bypass_mode: bool,
    #[serde(default)]
    pub additional_directories: Vec<String>,
    /// When true, only rules from policy settings are respected.
    /// TS: allowManagedPermissionRulesOnly
    #[serde(default)]
    pub allow_managed_permission_rules_only: bool,
}

/// Auto-mode/yolo classifier user configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModeConfig {
    pub allow: Vec<String>,
    pub soft_deny: Vec<String>,
    pub environment: Vec<String>,
}

/// An allowed MCP server entry in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedMcpServerEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// A denied MCP server entry in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeniedMcpServerEntry {
    pub name: String,
}

/// Plugin configuration in settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginConfig {
    pub enabled: bool,
}

/// Worktree configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorktreeConfig {
    pub enabled: bool,
}

/// Settings snapshot with per-source tracking.
#[derive(Debug, Clone)]
pub struct SettingsWithSource {
    pub merged: Settings,
    pub per_source: HashMap<SettingSource, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Load settings from a JSON string.
pub fn parse_settings(json: &str) -> anyhow::Result<Settings> {
    let settings: Settings = serde_json::from_str(json)?;
    Ok(settings)
}

/// Load and merge settings from multiple sources.
/// Merge order (later overrides earlier):
///   1. Plugin base
///   2. User global (~/.coco/settings.json)
///   3. Project shared (.claude/settings.json)
///   4. Project local (.claude/settings.local.json)
///   5. Flag (--settings file)
///   6. Policy (enterprise managed)
pub fn load_settings(
    cwd: &std::path::Path,
    flag_settings: Option<&std::path::Path>,
) -> anyhow::Result<SettingsWithSource> {
    use crate::global_config;

    let mut per_source = HashMap::new();
    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    // Load each source if it exists, merge in order
    let sources = [
        (SettingSource::User, global_config::user_settings_path()),
        (
            SettingSource::Project,
            global_config::project_settings_path(cwd),
        ),
        (
            SettingSource::Local,
            global_config::local_settings_path(cwd),
        ),
    ];

    for (source, path) in &sources {
        if path.exists()
            && let Ok(contents) = std::fs::read_to_string(path)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
        {
            per_source.insert(*source, value.clone());
            merge::deep_merge(&mut merged, &value);
        }
    }

    // Flag settings
    if let Some(flag_path) = flag_settings
        && flag_path.exists()
        && let Ok(contents) = std::fs::read_to_string(flag_path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
    {
        per_source.insert(SettingSource::Flag, value.clone());
        merge::deep_merge(&mut merged, &value);
    }

    // Policy settings
    let policy_path = global_config::managed_settings_path();
    if policy_path.exists()
        && let Ok(contents) = std::fs::read_to_string(&policy_path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
    {
        per_source.insert(SettingSource::Policy, value.clone());
        merge::deep_merge(&mut merged, &value);
    }

    let settings: Settings = serde_json::from_value(merged)?;

    Ok(SettingsWithSource {
        merged: settings,
        per_source,
    })
}
