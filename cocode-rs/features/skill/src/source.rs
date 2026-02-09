//! Skill source tracking.
//!
//! Each loaded skill carries provenance information describing where it
//! was discovered. This is used for precedence resolution and diagnostics.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

/// Where a skill was loaded from.
///
/// Skills can originate from built-in defaults, bundled skills, MCP servers,
/// plugins, project settings, user settings, or policy settings.
///
/// Variants are ordered by priority (lower number = lower priority).
/// When skills share the same name, higher-priority sources win.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    /// A built-in skill hardcoded in the binary (e.g., system commands).
    Builtin,

    /// A skill bundled with the binary.
    Bundled,

    /// A skill provided by an MCP server.
    Mcp,

    /// A skill provided by a plugin.
    Plugin {
        /// Name of the plugin that provided the skill.
        plugin_name: String,
    },

    /// A project-level skill from `.cocode/skills/` or project settings.
    ProjectSettings {
        /// Absolute path to the skill directory.
        path: PathBuf,
    },

    /// A user-level skill from `~/.cocode/skills/` or user settings.
    UserSettings {
        /// Absolute path to the skill directory.
        path: PathBuf,
    },

    /// A policy-level skill from organization policy settings.
    PolicySettings,
}

impl SkillSource {
    /// Returns the priority of this source (lower = lower priority).
    ///
    /// When skills share the same name, the source with higher priority wins.
    pub fn priority(&self) -> i32 {
        match self {
            Self::Builtin => 0,
            Self::Bundled => 1,
            Self::Mcp => 2,
            Self::Plugin { .. } => 3,
            Self::ProjectSettings { .. } => 4,
            Self::UserSettings { .. } => 5,
            Self::PolicySettings => 6,
        }
    }
}

impl fmt::Display for SkillSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin => write!(f, "builtin"),
            Self::Bundled => write!(f, "bundled"),
            Self::Mcp => write!(f, "mcp"),
            Self::ProjectSettings { path } => write!(f, "project-settings ({})", path.display()),
            Self::UserSettings { path } => write!(f, "user-settings ({})", path.display()),
            Self::Plugin { plugin_name } => write!(f, "plugin ({plugin_name})"),
            Self::PolicySettings => write!(f, "policy-settings"),
        }
    }
}

/// Categorization of where a skill was loaded from.
///
/// This is a simplified version of [`SkillSource`] used when the exact
/// path is not needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadedFrom {
    /// From built-in skills hardcoded in the binary.
    Builtin,

    /// From bundled skills compiled into the binary.
    Bundled,

    /// From an MCP server.
    Mcp,

    /// From a plugin directory.
    Plugin,

    /// From project-level skills directory.
    ProjectSettings,

    /// From user-level skills directory.
    UserSettings,

    /// From policy settings.
    PolicySettings,
}

impl fmt::Display for LoadedFrom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin => write!(f, "builtin"),
            Self::Bundled => write!(f, "bundled"),
            Self::Mcp => write!(f, "mcp"),
            Self::Plugin => write!(f, "plugin"),
            Self::ProjectSettings => write!(f, "project settings"),
            Self::UserSettings => write!(f, "user settings"),
            Self::PolicySettings => write!(f, "policy settings"),
        }
    }
}

impl From<&SkillSource> for LoadedFrom {
    fn from(source: &SkillSource) -> Self {
        match source {
            SkillSource::Builtin => Self::Builtin,
            SkillSource::Bundled => Self::Bundled,
            SkillSource::Mcp => Self::Mcp,
            SkillSource::Plugin { .. } => Self::Plugin,
            SkillSource::ProjectSettings { .. } => Self::ProjectSettings,
            SkillSource::UserSettings { .. } => Self::UserSettings,
            SkillSource::PolicySettings => Self::PolicySettings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_source_display() {
        assert_eq!(SkillSource::Builtin.to_string(), "builtin");
        assert_eq!(SkillSource::Bundled.to_string(), "bundled");
        assert_eq!(SkillSource::Mcp.to_string(), "mcp");
        assert_eq!(SkillSource::PolicySettings.to_string(), "policy-settings");

        let local = SkillSource::ProjectSettings {
            path: PathBuf::from("/project/.cocode/skills/commit"),
        };
        assert_eq!(
            local.to_string(),
            "project-settings (/project/.cocode/skills/commit)"
        );

        let global = SkillSource::UserSettings {
            path: PathBuf::from("/home/user/.cocode/skills/review"),
        };
        assert_eq!(
            global.to_string(),
            "user-settings (/home/user/.cocode/skills/review)"
        );

        let plugin = SkillSource::Plugin {
            plugin_name: "my-plugin".to_string(),
        };
        assert_eq!(plugin.to_string(), "plugin (my-plugin)");
    }

    #[test]
    fn test_loaded_from_display() {
        assert_eq!(LoadedFrom::Builtin.to_string(), "builtin");
        assert_eq!(LoadedFrom::Bundled.to_string(), "bundled");
        assert_eq!(LoadedFrom::Mcp.to_string(), "mcp");
        assert_eq!(LoadedFrom::Plugin.to_string(), "plugin");
        assert_eq!(LoadedFrom::ProjectSettings.to_string(), "project settings");
        assert_eq!(LoadedFrom::UserSettings.to_string(), "user settings");
        assert_eq!(LoadedFrom::PolicySettings.to_string(), "policy settings");
    }

    #[test]
    fn test_loaded_from_conversion() {
        assert_eq!(LoadedFrom::from(&SkillSource::Builtin), LoadedFrom::Builtin);
        assert_eq!(LoadedFrom::from(&SkillSource::Bundled), LoadedFrom::Bundled);
        assert_eq!(LoadedFrom::from(&SkillSource::Mcp), LoadedFrom::Mcp);
        assert_eq!(
            LoadedFrom::from(&SkillSource::ProjectSettings {
                path: PathBuf::from("/x")
            }),
            LoadedFrom::ProjectSettings
        );
        assert_eq!(
            LoadedFrom::from(&SkillSource::UserSettings {
                path: PathBuf::from("/x")
            }),
            LoadedFrom::UserSettings
        );
        assert_eq!(
            LoadedFrom::from(&SkillSource::Plugin {
                plugin_name: "p".to_string()
            }),
            LoadedFrom::Plugin
        );
        assert_eq!(
            LoadedFrom::from(&SkillSource::PolicySettings),
            LoadedFrom::PolicySettings
        );
    }

    #[test]
    fn test_priority_ordering() {
        assert!(SkillSource::Builtin.priority() < SkillSource::Bundled.priority());
        assert!(SkillSource::Bundled.priority() < SkillSource::Mcp.priority());
        assert!(
            SkillSource::Mcp.priority()
                < SkillSource::Plugin {
                    plugin_name: "x".to_string()
                }
                .priority()
        );
        assert!(
            SkillSource::Plugin {
                plugin_name: "x".to_string()
            }
            .priority()
                < SkillSource::ProjectSettings {
                    path: PathBuf::from("/x")
                }
                .priority()
        );
        assert!(
            SkillSource::ProjectSettings {
                path: PathBuf::from("/x")
            }
            .priority()
                < SkillSource::UserSettings {
                    path: PathBuf::from("/x")
                }
                .priority()
        );
        assert!(
            SkillSource::UserSettings {
                path: PathBuf::from("/x")
            }
            .priority()
                < SkillSource::PolicySettings.priority()
        );
    }
}
