//! Plugin scope definitions.
//!
//! Plugins are discovered from multiple scopes in priority order.

use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// The scope from which a plugin was loaded.
///
/// Scopes are ordered by priority (higher scopes override lower ones):
/// 1. Flag - `--plugin-dir` or inline (highest priority)
/// 2. Local - Development/local plugins
/// 3. Project - `.cocode/plugins/` in the project directory
/// 4. User - `~/.cocode/plugins/`
/// 5. Managed - System-installed plugins (lowest priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PluginScope {
    /// System-installed (lowest priority).
    Managed,

    /// User-global plugins.
    User,

    /// Project-local plugins.
    Project,

    /// Development/local plugins.
    Local,

    /// CLI flag or inline plugins (highest priority).
    Flag,
}

impl PluginScope {
    /// Get the default directory for this scope.
    pub fn default_dir(&self) -> Option<PathBuf> {
        match self {
            Self::Managed => {
                // Platform-specific system plugin directory
                #[cfg(target_os = "macos")]
                {
                    Some(PathBuf::from("/usr/local/share/cocode/plugins"))
                }
                #[cfg(target_os = "linux")]
                {
                    Some(PathBuf::from("/usr/share/cocode/plugins"))
                }
                #[cfg(target_os = "windows")]
                {
                    std::env::var("PROGRAMDATA")
                        .ok()
                        .map(|p| PathBuf::from(p).join("cocode").join("plugins"))
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                {
                    None
                }
            }
            Self::User | Self::Project | Self::Local | Self::Flag => {
                // These scopes depend on runtime context
                None
            }
        }
    }

    /// Get the priority of this scope (higher = more specific).
    pub fn priority(&self) -> i32 {
        match self {
            Self::Managed => 0,
            Self::User => 1,
            Self::Project => 2,
            Self::Local => 3,
            Self::Flag => 4,
        }
    }
}

impl std::fmt::Display for PluginScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Managed => write!(f, "managed"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::Local => write!(f, "local"),
            Self::Flag => write!(f, "flag"),
        }
    }
}

#[cfg(test)]
#[path = "scope.test.rs"]
mod tests;
