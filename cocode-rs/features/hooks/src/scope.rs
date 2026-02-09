//! Hook scope for priority ordering.
//!
//! Hooks are executed in scope priority order when multiple hooks match the
//! same event.

use serde::Deserialize;
use serde::Serialize;

/// The scope from which a hook originates, which determines its priority.
///
/// Lower numeric order = higher priority. `Policy` hooks always run first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    /// Organization-level policy hooks (highest priority).
    Policy = 0,
    /// Plugin-provided hooks.
    Plugin = 1,
    /// Session-level hooks.
    Session = 2,
    /// Skill-level hooks (lowest priority).
    Skill = 3,
}

/// The source of a hook, providing more detail than scope alone.
///
/// This identifies where a hook was registered from, enabling:
/// - Policy enforcement (only managed hooks)
/// - Cleanup when plugins/skills are unloaded
/// - Debugging and logging
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookSource {
    /// Registered by organization policy.
    Policy,

    /// Registered by a plugin.
    Plugin {
        /// The name of the plugin.
        name: String,
    },

    /// Registered for the current session.
    Session,

    /// Registered by a skill.
    Skill {
        /// The name of the skill.
        name: String,
    },
}

impl HookSource {
    /// Returns the scope for this source.
    pub fn scope(&self) -> HookScope {
        match self {
            Self::Policy => HookScope::Policy,
            Self::Plugin { .. } => HookScope::Plugin,
            Self::Session => HookScope::Session,
            Self::Skill { .. } => HookScope::Skill,
        }
    }

    /// Returns `true` if this source is a managed source (Policy or Plugin).
    ///
    /// Managed sources are allowed when `allow_managed_hooks_only` is enabled.
    pub fn is_managed(&self) -> bool {
        matches!(self, Self::Policy | Self::Plugin { .. })
    }

    /// Returns the name associated with this source, if any.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Policy | Self::Session => None,
            Self::Plugin { name } | Self::Skill { name } => Some(name),
        }
    }
}

impl Default for HookSource {
    fn default() -> Self {
        Self::Session
    }
}

impl std::fmt::Display for HookSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy => write!(f, "policy"),
            Self::Plugin { name } => write!(f, "plugin:{name}"),
            Self::Session => write!(f, "session"),
            Self::Skill { name } => write!(f, "skill:{name}"),
        }
    }
}

impl HookScope {
    /// Returns the string representation of this scope.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Plugin => "plugin",
            Self::Session => "session",
            Self::Skill => "skill",
        }
    }
}

impl std::fmt::Display for HookScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Ord for HookScope {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as i32).cmp(&(*other as i32))
    }
}

impl PartialOrd for HookScope {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
#[path = "scope.test.rs"]
mod tests;
