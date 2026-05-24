//! Plugin contribution types.
//!
//! Plugins can contribute various types of extensions to cocode:
//! - Skills (slash commands)
//! - Hooks (lifecycle interceptors)
//! - Agents (specialized subagents)
//! - Commands (plugin-provided commands)
//! - MCP servers (Model Context Protocol servers)
//! - LSP servers (Language Server Protocol servers)

use cocode_hooks::HookDefinition;
use cocode_skill::SkillPromptCommand;
use cocode_subagent::AgentDefinition;
use serde::Deserialize;
use serde::Serialize;

use crate::command::PluginCommand;
use crate::lsp_loader::LspServerConfig;
use crate::mcp::McpServerConfig;

/// A flexible type that accepts either a single string or an array of strings
/// in JSON manifests.
///
/// This allows plugin authors to write either:
/// ```json
/// { "skills": "skills/" }
/// ```
/// or:
/// ```json
/// { "skills": ["skills/", "more-skills/"] }
/// ```
#[derive(Debug, Clone, Default, Serialize)]
pub struct StringOrVec(pub Vec<String>);

impl<'de> Deserialize<'de> for StringOrVec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StringOrVecVisitor;

        impl<'de> serde::de::Visitor<'de> for StringOrVecVisitor {
            type Value = StringOrVec;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an array of strings")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<StringOrVec, E>
            where
                E: serde::de::Error,
            {
                Ok(StringOrVec(vec![value.to_string()]))
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<StringOrVec, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    vec.push(s);
                }
                Ok(StringOrVec(vec))
            }
        }

        deserializer.deserialize_any(StringOrVecVisitor)
    }
}

impl std::ops::Deref for StringOrVec {
    type Target = Vec<String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for StringOrVec {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<Vec<String>> for StringOrVec {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Vec<&str>> for StringOrVec {
    fn eq(&self, other: &Vec<&str>) -> bool {
        if self.0.len() != other.len() {
            return false;
        }
        self.0.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl From<Vec<String>> for StringOrVec {
    fn from(v: Vec<String>) -> Self {
        Self(v)
    }
}

impl<'a> IntoIterator for &'a StringOrVec {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Contributions declared in a plugin manifest.
///
/// Each field is a list of paths (relative to the plugin directory) that
/// contain contribution definitions. Fields accept either a single string
/// or an array of strings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginContributions {
    /// Paths to skill directories (containing SKILL.md files).
    #[serde(default)]
    pub skills: StringOrVec,

    /// Paths to hook configuration files (JSON).
    #[serde(default)]
    pub hooks: StringOrVec,

    /// Paths to agent directories (containing agent.json files).
    #[serde(default)]
    pub agents: StringOrVec,

    /// Paths to command directories (containing command.json files).
    #[serde(default)]
    pub commands: StringOrVec,

    /// Paths to MCP server configuration files.
    #[serde(default)]
    pub mcp_servers: StringOrVec,

    /// Paths to LSP server configuration files.
    #[serde(default)]
    pub lsp_servers: StringOrVec,

    /// Paths to output style directories.
    #[serde(default)]
    pub output_styles: StringOrVec,
}

/// A contribution from a plugin.
///
/// This represents a loaded contribution with its source plugin tracked.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum PluginContribution {
    /// A skill contribution.
    Skill {
        /// The loaded skill.
        skill: SkillPromptCommand,
        /// The plugin that contributed this skill.
        plugin_name: String,
    },

    /// A hook contribution.
    Hook {
        /// The loaded hook definition.
        hook: HookDefinition,
        /// The plugin that contributed this hook.
        plugin_name: String,
    },

    /// An agent contribution.
    Agent {
        /// The loaded agent definition.
        definition: AgentDefinition,
        /// The plugin that contributed this agent.
        plugin_name: String,
    },

    /// A command contribution.
    Command {
        /// The loaded command.
        command: PluginCommand,
        /// The plugin that contributed this command.
        plugin_name: String,
    },

    /// An MCP server contribution.
    McpServer {
        /// The MCP server configuration.
        config: McpServerConfig,
        /// The plugin that contributed this server.
        plugin_name: String,
    },

    /// An LSP server contribution.
    LspServer {
        /// The LSP server configuration.
        config: LspServerConfig,
        /// The plugin that contributed this server.
        plugin_name: String,
    },

    /// An output style contribution.
    OutputStyle {
        /// The output style definition.
        style: OutputStyleDefinition,
        /// The plugin that contributed this style.
        plugin_name: String,
    },
}

/// An output style definition loaded from a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputStyleDefinition {
    /// Style name (derived from the filename).
    pub name: String,
    /// The style prompt content (loaded from .md file).
    pub prompt: String,
}

impl PluginContribution {
    /// Get the name of this contribution.
    pub fn name(&self) -> &str {
        match self {
            Self::Skill { skill, .. } => &skill.name,
            Self::Hook { hook, .. } => &hook.name,
            Self::Agent { definition, .. } => &definition.name,
            Self::Command { command, .. } => &command.name,
            Self::McpServer { config, .. } => &config.name,
            Self::LspServer { config, .. } => &config.name,
            Self::OutputStyle { style, .. } => &style.name,
        }
    }

    /// Get the plugin that contributed this.
    pub fn plugin_name(&self) -> &str {
        match self {
            Self::Skill { plugin_name, .. }
            | Self::Hook { plugin_name, .. }
            | Self::Agent { plugin_name, .. }
            | Self::Command { plugin_name, .. }
            | Self::McpServer { plugin_name, .. }
            | Self::LspServer { plugin_name, .. }
            | Self::OutputStyle { plugin_name, .. } => plugin_name,
        }
    }

    /// Check if this is a skill contribution.
    pub fn is_skill(&self) -> bool {
        matches!(self, Self::Skill { .. })
    }

    /// Check if this is a hook contribution.
    pub fn is_hook(&self) -> bool {
        matches!(self, Self::Hook { .. })
    }

    /// Check if this is an agent contribution.
    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    /// Check if this is a command contribution.
    pub fn is_command(&self) -> bool {
        matches!(self, Self::Command { .. })
    }

    /// Check if this is an MCP server contribution.
    pub fn is_mcp_server(&self) -> bool {
        matches!(self, Self::McpServer { .. })
    }

    /// Check if this is an LSP server contribution.
    pub fn is_lsp_server(&self) -> bool {
        matches!(self, Self::LspServer { .. })
    }

    /// Check if this is an output style contribution.
    pub fn is_output_style(&self) -> bool {
        matches!(self, Self::OutputStyle { .. })
    }
}

#[cfg(test)]
#[path = "contribution.test.rs"]
mod tests;
