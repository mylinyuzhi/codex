//! Global hook settings.

use serde::Deserialize;
use serde::Serialize;

/// Global settings that control hook behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSettings {
    /// Disable all hooks globally.
    #[serde(default)]
    pub disable_all_hooks: bool,

    /// Only allow hooks from managed (policy/plugin) sources.
    #[serde(default)]
    pub allow_managed_hooks_only: bool,

    /// Whether the workspace is trusted.
    ///
    /// When `false`, only managed hooks (Policy/Plugin sources) are executed.
    /// Non-managed hooks (Session, Agent, Skill) are filtered out.
    /// Defaults to `true` for backward compatibility.
    #[serde(default = "default_workspace_trusted")]
    pub workspace_trusted: bool,
}

fn default_workspace_trusted() -> bool {
    true
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
            workspace_trusted: true,
        }
    }
}

#[cfg(test)]
#[path = "settings.test.rs"]
mod tests;
