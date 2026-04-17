//! Agent spawning infrastructure.
//!
//! TS: tools/AgentTool/runAgent.ts (35.7K), forkSubagent.ts (8.6K),
//! loadAgentsDir.ts (26.2K)
//!
//! Provides agent definition loading, tool pool assembly, and
//! the spawn lifecycle (fork, worktree isolation, background execution).
//!
//! Uses the canonical `coco_types::AgentDefinition` as the single source
//! of truth for agent specs. Discovery functions here parse files and
//! construct that type.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

// Re-export the canonical type so consumers import from here.
pub use coco_types::AgentDefinition;
use coco_types::AgentTypeId;
use coco_types::SubagentType;

/// Built-in agent type identifiers.
///
/// TS: ONE_SHOT_BUILTIN_AGENT_TYPES + GENERAL_PURPOSE_AGENT + builtInAgents.ts
pub const BUILTIN_AGENT_TYPES: &[&str] = &[
    "general-purpose",
    "Explore",
    "Plan",
    "claude-code-guide",
    "statusline-setup",
    "verification",
];

/// One-shot agent types that skip usage trailer and SendMessage continuation.
///
/// TS: ONE_SHOT_BUILTIN_AGENT_TYPES in constants.ts
pub const ONE_SHOT_AGENT_TYPES: &[&str] = &["Explore", "Plan"];

/// Agent spawn configuration (parameters from the AgentTool call).
#[derive(Debug, Clone)]
pub struct AgentSpawnConfig {
    pub prompt: String,
    pub description: Option<String>,
    pub model: Option<String>,
    pub subagent_type: Option<String>,
    pub isolation: AgentIsolation,
    pub run_in_background: bool,
    pub working_dir: Option<PathBuf>,
}

/// Agent isolation mode (local to the spawn infrastructure).
#[derive(Debug, Clone, Default)]
pub enum AgentIsolation {
    /// Run in the same process (shared context).
    #[default]
    InProcess,
    /// Run in a git worktree (isolated filesystem).
    Worktree { branch: Option<String> },
}

/// Agent status tracking (local to spawn lifecycle).
#[derive(Debug, Clone)]
pub enum AgentStatus {
    Pending,
    Running { agent_id: String },
    Completed { result: String },
    Failed { error: String },
    Backgrounded { agent_id: String },
}

/// Upper bound on the size of a single agent-definition markdown file.
/// The parser reads the entire file into memory; anything larger than
/// this is almost certainly unintended and would waste memory / CPU
/// on an adversary-controlled file in `~/.coco/agents/`.
const MAX_AGENT_FILE_BYTES: u64 = 1_048_576; // 1 MiB

/// Load agent definitions from directories.
///
/// TS: loadAgentsDir() — walks directories for .md agent definition files.
///
/// Security hardening:
/// - `follow_links(false)` — symlinks inside agent dirs are ignored so
///   a `~/.coco/agents/escape -> /etc` link can't cause the parser to
///   read files outside the user-controlled agent tree.
/// - `MAX_AGENT_FILE_BYTES` — files larger than 1 MiB are skipped
///   silently to bound memory usage. A realistic agent definition is
///   a few KiB of YAML + markdown.
pub fn load_agents_from_dirs(dirs: &[PathBuf]) -> Vec<AgentDefinition> {
    let mut agents = Vec::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .max_depth(2)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e == "md") || !path.is_file() {
                continue;
            }
            // Skip oversized files before the parser touches them.
            if let Ok(meta) = std::fs::metadata(path)
                && meta.len() > MAX_AGENT_FILE_BYTES
            {
                continue;
            }
            if let Some(agent) = parse_agent_definition(path) {
                agents.push(agent);
            }
        }
    }

    agents
}

/// Standard agent definition directories.
pub fn get_agent_dirs(config_dir: &Path, project_dir: &Path) -> Vec<PathBuf> {
    vec![
        config_dir.join("agents"),
        project_dir.join(".claude").join("agents"),
    ]
}

/// Parse an agent definition from a markdown file.
///
/// Format: YAML frontmatter with name, description, allowed_tools, etc.
/// Body is the agent's system prompt / initial_prompt.
fn parse_agent_definition(path: &Path) -> Option<AgentDefinition> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();

    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Try to find YAML frontmatter
    let first_line = lines.next()?.trim();
    if first_line != "---" {
        // No frontmatter — treat entire file as prompt with filename as name
        return Some(AgentDefinition {
            agent_type: AgentTypeId::Custom(file_name.clone()),
            name: file_name,
            initial_prompt: Some(content),
            ..Default::default()
        });
    }

    // Parse frontmatter
    let mut frontmatter = HashMap::new();
    let mut body_start = 0;
    for (i, line) in content.lines().enumerate().skip(1) {
        if line.trim() == "---" {
            body_start = i + 1;
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            frontmatter.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let body: String = content
        .lines()
        .skip(body_start)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    let name = frontmatter.get("name").cloned().unwrap_or(file_name);

    let parse_list = |key: &str| -> Vec<String> {
        frontmatter
            .get(key)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    // Resolve agent_type from name
    let agent_type = resolve_agent_type(&name);

    Some(AgentDefinition {
        agent_type,
        name,
        description: frontmatter.get("description").cloned(),
        initial_prompt: if body.is_empty() { None } else { Some(body) },
        allowed_tools: parse_list("allowed_tools"),
        disallowed_tools: parse_list("disallowed_tools"),
        mcp_servers: parse_list("required_mcp_servers"),
        max_turns: frontmatter.get("max_turns").and_then(|v| v.parse().ok()),
        model: frontmatter.get("model").cloned(),
        ..Default::default()
    })
}

/// Resolve an agent name to a typed AgentTypeId.
pub fn resolve_agent_type(name: &str) -> AgentTypeId {
    match name.parse::<SubagentType>() {
        Ok(builtin) => AgentTypeId::Builtin(builtin),
        Err(_) => {
            // Check alternate names used in definitions
            match name {
                "general-purpose" => AgentTypeId::Custom("general-purpose".into()),
                _ => AgentTypeId::Custom(name.into()),
            }
        }
    }
}

/// Filter agents by available MCP servers.
///
/// Uses `required_mcp_servers` (patterns that must match available servers).
/// Case-insensitive substring matching per TS `hasRequiredMcpServers()`.
///
/// TS: filterAgentsByMcpRequirements()
pub fn filter_agents_by_mcp(
    agents: &[AgentDefinition],
    available_mcp_servers: &[String],
) -> Vec<AgentDefinition> {
    agents
        .iter()
        .filter(|a| {
            a.required_mcp_servers.is_empty()
                || a.required_mcp_servers.iter().all(|pattern| {
                    let pattern_lower = pattern.to_lowercase();
                    available_mcp_servers
                        .iter()
                        .any(|server| server.to_lowercase().contains(&pattern_lower))
                })
        })
        .cloned()
        .collect()
}

/// Check if an agent name refers to a built-in agent type.
pub fn is_builtin_agent(name: &str) -> bool {
    BUILTIN_AGENT_TYPES.contains(&name)
}

/// Check if an AgentDefinition represents a built-in agent.
pub fn is_builtin_definition(def: &AgentDefinition) -> bool {
    matches!(def.agent_type, AgentTypeId::Builtin(_))
}

/// Get the built-in general-purpose agent definition.
pub fn general_purpose_agent() -> AgentDefinition {
    AgentDefinition {
        agent_type: AgentTypeId::Custom("general-purpose".into()),
        name: "general-purpose".to_string(),
        description: Some(
            "General-purpose agent for research, search, and multi-step tasks.".to_string(),
        ),
        max_turns: Some(30),
        ..Default::default()
    }
}

/// Return the full set of built-in agent definitions shipped with
/// coco-rs. Used by the SDK server's `initialize` response to advertise
/// available agents to clients before any user-defined markdown files
/// are discovered.
///
/// Descriptions are deliberately short and task-focused; individual
/// agents can be enriched later by placing override markdown under
/// `~/.coco/agents/` which `load_agents_from_dirs` will merge on top
/// of these defaults by name.
pub fn builtin_agents() -> Vec<AgentDefinition> {
    vec![
        general_purpose_agent(),
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::Explore),
            name: "Explore".to_string(),
            description: Some(
                "Fast codebase explorer — finds files, searches content, answers \
                 structural questions without modifying state."
                    .to_string(),
            ),
            max_turns: Some(15),
            ..Default::default()
        },
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::Plan),
            name: "Plan".to_string(),
            description: Some(
                "Software architect — designs implementation strategies and returns \
                 step-by-step plans without writing code."
                    .to_string(),
            ),
            max_turns: Some(20),
            ..Default::default()
        },
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::Review),
            name: "Review".to_string(),
            description: Some(
                "Code review agent — inspects diffs and proposed changes for \
                 correctness, style, and hidden regressions."
                    .to_string(),
            ),
            max_turns: Some(15),
            ..Default::default()
        },
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::StatusLine),
            name: "statusline-setup".to_string(),
            description: Some(
                "Configures the CLI status line — keybindings, prompts, and \
                 appearance in `~/.coco/statusline.json`."
                    .to_string(),
            ),
            max_turns: Some(5),
            ..Default::default()
        },
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::ClaudeCodeGuide),
            name: "claude-code-guide".to_string(),
            description: Some(
                "Reference assistant for Claude Code / Anthropic SDK / Claude API \
                 usage questions."
                    .to_string(),
            ),
            max_turns: Some(10),
            ..Default::default()
        },
    ]
}

#[cfg(test)]
#[path = "agent_spawn.test.rs"]
mod tests;
