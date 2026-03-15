//! Global hook settings.

use serde::Deserialize;
use serde::Serialize;

/// Global settings that control hook behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookSettings {
    /// Disable all hooks globally.
    #[serde(default)]
    pub disable_all_hooks: bool,

    /// Only allow hooks from managed (policy/plugin) sources.
    #[serde(default)]
    pub allow_managed_hooks_only: bool,
}

#[cfg(test)]
#[path = "settings.test.rs"]
mod tests;
