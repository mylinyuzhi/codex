//! Custom agent loader from Markdown files with YAML frontmatter.
//!
//! Scans directories for `.md` files that define custom agents. Each file
//! consists of YAML frontmatter (delimited by `---`) followed by a Markdown
//! body that serves as the agent's description/system prompt.
//!
//! # File Format
//!
//! ```markdown
//! ---
//! name: my-agent
//! description: A custom agent that does X
//! model: fast
//! tools:
//!   - Read
//!   - Glob
//!   - Grep
//! disallowedTools:
//!   - Edit
//! maxTurns: 15
//! permissionMode: bypass
//! forkContext: false
//! color: cyan
//! ---
//! Body text becomes the agent's critical_reminder / system prompt injection.
//! ```
//!
//! # Scan Directories
//!
//! - User agents: `~/.cocode/agents/`
//! - Project agents: `.cocode/agents/` (relative to project root)

use std::path::Path;

use serde::Deserialize;

use crate::definition::AgentDefinition;
use crate::definition::AgentSource;

/// YAML frontmatter schema for agent definition files.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFrontmatter {
    /// Agent name / type identifier. Defaults to filename stem.
    #[serde(default)]
    name: Option<String>,

    /// Human-readable description.
    #[serde(default)]
    description: Option<String>,

    /// Model role: "main", "fast", "explore", "plan", "vision", "review", "compact", "inherit".
    #[serde(default)]
    model: Option<String>,

    /// Allowed tools (empty or absent = all tools available).
    #[serde(default)]
    tools: Option<Vec<String>>,

    /// Explicitly denied tools.
    #[serde(default)]
    disallowed_tools: Option<Vec<String>>,

    /// Maximum number of turns.
    #[serde(default)]
    max_turns: Option<i32>,

    /// Permission mode: "default", "plan", "acceptEdits", "bypass", "dontAsk".
    #[serde(default)]
    permission_mode: Option<String>,

    /// Whether to fork parent conversation context.
    #[serde(default)]
    fork_context: Option<bool>,

    /// TUI display color.
    #[serde(default)]
    color: Option<String>,
}

/// Load custom agent definitions from a directory of Markdown files.
///
/// Each `.md` file in the directory is parsed as an agent definition.
/// Files without valid frontmatter are skipped with a warning.
///
/// Returns agent definitions with the given `source` tag.
pub fn load_agents_from_dir(dir: &Path, source: AgentSource) -> Vec<AgentDefinition> {
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut agents = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!(
                path = %dir.display(),
                error = %e,
                "Failed to read agents directory"
            );
            return Vec::new();
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to read directory entry");
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        match load_agent_from_file(&path, source) {
            Ok(def) => {
                tracing::debug!(
                    agent_type = %def.agent_type,
                    source = ?source,
                    path = %path.display(),
                    "Loaded custom agent definition"
                );
                agents.push(def);
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to load agent definition, skipping"
                );
            }
        }
    }

    agents
}

/// Load a single agent definition from a Markdown file.
fn load_agent_from_file(path: &Path, source: AgentSource) -> anyhow::Result<AgentDefinition> {
    let content = std::fs::read_to_string(path)?;
    let (yaml_str, body) =
        parse_frontmatter(&content).map_err(|e| anyhow::anyhow!("frontmatter parse error: {e}"))?;

    let fm: AgentFrontmatter =
        serde_yml::from_str(yaml_str).map_err(|e| anyhow::anyhow!("YAML parse error: {e}"))?;

    // Derive agent_type from the `name` field or the filename stem
    let agent_type = fm
        .name
        .clone()
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
        .ok_or_else(|| anyhow::anyhow!("cannot determine agent name from file"))?;

    let description = fm.description.unwrap_or_default();
    let body_trimmed = body.trim();
    let critical_reminder = if body_trimmed.is_empty() {
        None
    } else {
        Some(body_trimmed.to_string())
    };

    let identity = fm.model.as_deref().map(parse_identity);
    let permission_mode = fm.permission_mode.as_deref().map(parse_permission_mode);

    Ok(AgentDefinition {
        name: agent_type.clone(),
        description,
        agent_type,
        tools: fm.tools.unwrap_or_default(),
        disallowed_tools: fm.disallowed_tools.unwrap_or_default(),
        identity,
        max_turns: fm.max_turns,
        permission_mode,
        fork_context: fm.fork_context.unwrap_or(false),
        color: fm.color,
        critical_reminder,
        source,
    })
}

/// Parse YAML frontmatter from a markdown string.
///
/// Splits on `---` delimiters at line starts. Returns `(yaml_str, body_str)`.
fn parse_frontmatter(content: &str) -> Result<(&str, &str), String> {
    let content = content.trim_start_matches('\u{feff}');
    let rest = if content.starts_with("---") {
        &content[3..]
    } else {
        return Err("missing opening `---` frontmatter delimiter".to_string());
    };

    let rest = match rest.find('\n') {
        Some(pos) => &rest[pos + 1..],
        None => return Err("frontmatter is empty (no closing `---`)".to_string()),
    };

    // Find closing `---` on its own line
    let mut pos = 0;
    for line in rest.lines() {
        if line.trim() == "---" {
            let yaml_str = &rest[..pos];
            let after = &rest[pos + line.len()..];
            let body = match after.find('\n') {
                Some(p) => &after[p + 1..],
                None => "",
            };
            return Ok((yaml_str, body));
        }
        pos += line.len() + 1;
    }

    Err("missing closing `---` frontmatter delimiter".to_string())
}

/// Parse a model identity string.
fn parse_identity(s: &str) -> cocode_protocol::execution::ExecutionIdentity {
    use cocode_protocol::execution::ExecutionIdentity;
    use cocode_protocol::model::ModelRole;

    match s.to_lowercase().as_str() {
        "main" => ExecutionIdentity::Role(ModelRole::Main),
        "fast" | "haiku" => ExecutionIdentity::Role(ModelRole::Fast),
        "explore" => ExecutionIdentity::Role(ModelRole::Explore),
        "plan" => ExecutionIdentity::Role(ModelRole::Plan),
        "vision" => ExecutionIdentity::Role(ModelRole::Vision),
        "review" => ExecutionIdentity::Role(ModelRole::Review),
        "compact" => ExecutionIdentity::Role(ModelRole::Compact),
        "inherit" | _ => ExecutionIdentity::Inherit,
    }
}

/// Parse a permission mode string.
fn parse_permission_mode(s: &str) -> cocode_protocol::PermissionMode {
    use cocode_protocol::PermissionMode;

    match s.to_lowercase().as_str() {
        "bypass" => PermissionMode::Bypass,
        "dontask" | "dont_ask" | "dont-ask" => PermissionMode::DontAsk,
        "plan" => PermissionMode::Plan,
        "acceptedits" | "accept_edits" | "accept-edits" => PermissionMode::AcceptEdits,
        _ => PermissionMode::Default,
    }
}

/// Load custom agents from both user and project directories.
///
/// User agents come from `{cocode_home}/agents/`.
/// Project agents come from `{project_root}/.cocode/agents/`.
///
/// Project agents take priority over user agents when names conflict
/// (later entries in the returned Vec override earlier ones).
pub fn load_custom_agents(cocode_home: &Path, project_root: Option<&Path>) -> Vec<AgentDefinition> {
    let mut agents = Vec::new();

    // User agents (lower priority)
    let user_dir = cocode_home.join("agents");
    agents.extend(load_agents_from_dir(&user_dir, AgentSource::UserSettings));

    // Project agents (higher priority)
    if let Some(root) = project_root {
        let project_dir = root.join(".cocode").join("agents");
        agents.extend(load_agents_from_dir(
            &project_dir,
            AgentSource::ProjectSettings,
        ));
    }

    agents
}

/// Merge custom agents into a list of existing definitions.
///
/// Custom agents with the same `agent_type` as an existing definition
/// replace the existing one. New agent types are appended.
pub fn merge_custom_agents(existing: &mut Vec<AgentDefinition>, custom: Vec<AgentDefinition>) {
    for agent in custom {
        if let Some(pos) = existing
            .iter()
            .position(|d| d.agent_type == agent.agent_type)
        {
            tracing::debug!(
                agent_type = %agent.agent_type,
                source = ?agent.source,
                "Custom agent overrides existing definition"
            );
            existing[pos] = agent;
        } else {
            tracing::debug!(
                agent_type = %agent.agent_type,
                source = ?agent.source,
                "Adding new custom agent"
            );
            existing.push(agent);
        }
    }
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
