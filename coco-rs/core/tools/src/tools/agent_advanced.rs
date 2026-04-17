//! Advanced agent features ported from TS AgentTool/.
//!
//! TS: tools/AgentTool/agentToolUtils.ts, agentColorManager.ts,
//! loadAgentsDir.ts, runAgent.ts
//!
//! Provides agent discovery from directories, tool pool assembly for
//! sub-agents (filter allowed/disallowed), system prompt enhancement,
//! background agent execution tracking, result summarization, and
//! agent color assignment.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use coco_types::MCP_TOOL_PREFIX;
use coco_types::ToolName;
use tokio::sync::Mutex;

use coco_types::AgentDefinition;
use coco_types::AgentTypeId;

use super::agent_spawn::is_builtin_definition;

// ── Agent color management ──
// TS: agentColorManager.ts

/// Available agent colors for UI differentiation.
///
/// TS: AGENT_COLORS
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentColor {
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}

impl AgentColor {
    /// All available colors in assignment order.
    pub const ALL: &[AgentColor] = &[
        AgentColor::Red,
        AgentColor::Blue,
        AgentColor::Green,
        AgentColor::Yellow,
        AgentColor::Purple,
        AgentColor::Orange,
        AgentColor::Pink,
        AgentColor::Cyan,
    ];

    /// Color name as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentColor::Red => "red",
            AgentColor::Blue => "blue",
            AgentColor::Green => "green",
            AgentColor::Yellow => "yellow",
            AgentColor::Purple => "purple",
            AgentColor::Orange => "orange",
            AgentColor::Pink => "pink",
            AgentColor::Cyan => "cyan",
        }
    }
}

/// Manages color assignments for agents.
///
/// TS: agentColorManager — assigns unique colors to agent types.
#[derive(Debug, Clone)]
pub struct AgentColorManager {
    assignments: Arc<Mutex<HashMap<String, AgentColor>>>,
}

impl Default for AgentColorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentColorManager {
    pub fn new() -> Self {
        Self {
            assignments: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get or assign a color for an agent type.
    /// "general-purpose" agents get no color (returns None).
    ///
    /// TS: getAgentColor() + setAgentColor()
    pub async fn get_or_assign(&self, agent_type: &str) -> Option<AgentColor> {
        if agent_type == "general-purpose" {
            return None;
        }

        let mut assignments = self.assignments.lock().await;

        if let Some(color) = assignments.get(agent_type) {
            return Some(*color);
        }

        // Assign next available color (round-robin)
        let used_colors: HashSet<AgentColor> = assignments.values().copied().collect();
        let next_color = AgentColor::ALL
            .iter()
            .find(|c| !used_colors.contains(c))
            .copied()
            // If all colors are used, cycle from the beginning
            .unwrap_or(AgentColor::ALL[assignments.len() % AgentColor::ALL.len()]);

        assignments.insert(agent_type.to_string(), next_color);
        Some(next_color)
    }

    /// Remove a color assignment.
    pub async fn remove(&self, agent_type: &str) {
        let mut assignments = self.assignments.lock().await;
        assignments.remove(agent_type);
    }
}

// ── Tool pool assembly ──
// TS: agentToolUtils.ts — filterToolsForAgent(), resolveAgentTools()

/// Tools that are never available to any sub-agent.
///
/// TS: ALL_AGENT_DISALLOWED_TOOLS
pub const ALL_AGENT_DISALLOWED_TOOLS: &[&str] = &[
    ToolName::TeamCreate.as_str(),
    ToolName::TeamDelete.as_str(),
    ToolName::SendMessage.as_str(),
    ToolName::CronCreate.as_str(),
    ToolName::CronDelete.as_str(),
    ToolName::CronList.as_str(),
    ToolName::RemoteTrigger.as_str(),
];

/// Additional tools disallowed for custom (non-builtin) agents.
///
/// TS: CUSTOM_AGENT_DISALLOWED_TOOLS
pub const CUSTOM_AGENT_DISALLOWED_TOOLS: &[&str] = &[ToolName::AskUserQuestion.as_str()];

/// Tools allowed for async (background) agents.
///
/// TS: ASYNC_AGENT_ALLOWED_TOOLS
pub const ASYNC_AGENT_ALLOWED_TOOLS: &[&str] = &[
    ToolName::Bash.as_str(),
    ToolName::Read.as_str(),
    ToolName::Write.as_str(),
    ToolName::Edit.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Grep.as_str(),
    ToolName::WebFetch.as_str(),
    ToolName::WebSearch.as_str(),
    ToolName::Lsp.as_str(),
    ToolName::NotebookEdit.as_str(),
];

/// Result of resolving tools for an agent.
///
/// TS: ResolvedAgentTools
#[derive(Debug, Clone)]
pub struct ResolvedAgentTools {
    /// Whether the agent uses wildcard (all tools).
    pub has_wildcard: bool,
    /// Tool names that were valid and resolved.
    pub valid_tools: Vec<String>,
    /// Tool names from the agent def that didn't match any available tool.
    pub invalid_tools: Vec<String>,
    /// Resolved tool names after filtering.
    pub resolved_tool_names: Vec<String>,
    /// Allowed agent types for nested spawning (from Agent tool spec).
    pub allowed_agent_types: Option<Vec<String>>,
}

/// Filter available tools for a sub-agent.
///
/// TS: filterToolsForAgent() in agentToolUtils.ts
///
/// Rules (in order):
/// 1. MCP tools (mcp__ prefix) — always allowed
/// 2. ExitPlanMode allowed when permission_mode is "plan"
/// 3. ALL_AGENT_DISALLOWED_TOOLS — universal blocklist
/// 4. CUSTOM_AGENT_DISALLOWED_TOOLS — extra blocks for non-built-in agents
/// 5. Async agents have a specific allowlist (with in-process teammate exception)
pub fn filter_tools_for_agent(
    available_tools: &[String],
    is_builtin: bool,
    is_async: bool,
) -> Vec<String> {
    filter_tools_for_agent_with_options(available_tools, is_builtin, is_async, None, false)
}

/// Extended tool filtering with plan mode and teammate awareness.
///
/// TS: filterToolsForAgent({ tools, isBuiltIn, isAsync, permissionMode })
pub fn filter_tools_for_agent_with_options(
    available_tools: &[String],
    is_builtin: bool,
    is_async: bool,
    permission_mode: Option<&str>,
    is_in_process_teammate: bool,
) -> Vec<String> {
    let disallowed: HashSet<&str> = ALL_AGENT_DISALLOWED_TOOLS.iter().copied().collect();
    let custom_disallowed: HashSet<&str> = CUSTOM_AGENT_DISALLOWED_TOOLS.iter().copied().collect();
    let async_allowed: HashSet<&str> = ASYNC_AGENT_ALLOWED_TOOLS.iter().copied().collect();

    available_tools
        .iter()
        .filter(|tool| {
            let name = tool.as_str();
            // MCP tools always allowed
            if name.starts_with(MCP_TOOL_PREFIX) {
                return true;
            }
            // ExitPlanMode allowed when permission mode is "plan"
            if name == ToolName::ExitPlanMode.as_str() && permission_mode == Some("plan") {
                return true;
            }
            // Global disallowed list
            if disallowed.contains(name) {
                return false;
            }
            // Custom agent restrictions
            if !is_builtin && custom_disallowed.contains(name) {
                return false;
            }
            // Async agents have a specific allowlist
            if is_async && !async_allowed.contains(name) {
                // Exception: in-process teammates can use Agent tool + task tools
                if is_in_process_teammate
                    && (name == ToolName::Agent.as_str()
                        || name == ToolName::TaskCreate.as_str()
                        || name == ToolName::TaskUpdate.as_str()
                        || name == ToolName::TaskGet.as_str()
                        || name == ToolName::TaskList.as_str())
                {
                    return true;
                }
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

/// Parse an agent tool spec like "Agent(worker, researcher)" into (tool_name, allowed_types).
///
/// TS: permissionRuleValueFromString() → extracts toolName + ruleContent
pub fn parse_tool_spec(spec: &str) -> (&str, Option<Vec<&str>>) {
    if let Some(paren_start) = spec.find('(')
        && let Some(paren_end) = spec.find(')')
    {
        let tool_name = spec[..paren_start].trim();
        let content = &spec[paren_start + 1..paren_end];
        let types: Vec<&str> = content.split(',').map(str::trim).collect();
        return (tool_name, Some(types));
    }
    (spec.trim(), None)
}

/// Resolve and validate agent tools against available tools.
///
/// TS: resolveAgentTools() in agentToolUtils.ts
///
/// Supports tool specs like `"Agent(worker, researcher)"` which extract
/// `allowedAgentTypes` from the parenthesized content.
pub fn resolve_agent_tools(
    agent: &AgentDefinition,
    available_tools: &[String],
    is_async: bool,
) -> ResolvedAgentTools {
    let filtered = filter_tools_for_agent(available_tools, is_builtin_definition(agent), is_async);

    // Build disallowed set (parse "Tool(pattern)" syntax)
    let disallowed_set: HashSet<&str> = agent
        .disallowed_tools
        .iter()
        .map(|spec| parse_tool_spec(spec).0)
        .collect();

    // Remove disallowed tools
    let allowed: Vec<String> = filtered
        .into_iter()
        .filter(|t| !disallowed_set.contains(t.as_str()))
        .collect();

    // Wildcard: no explicit allowed_tools or ["*"]
    let has_wildcard = agent.allowed_tools.is_empty()
        || (agent.allowed_tools.len() == 1 && agent.allowed_tools[0] == "*");

    if has_wildcard {
        return ResolvedAgentTools {
            has_wildcard: true,
            valid_tools: Vec::new(),
            invalid_tools: Vec::new(),
            resolved_tool_names: allowed,
            allowed_agent_types: None,
        };
    }

    // Resolve specific tool names (with Agent(types) parsing)
    let available_set: HashSet<&str> = allowed.iter().map(String::as_str).collect();
    let mut valid_tools = Vec::new();
    let mut invalid_tools = Vec::new();
    let mut resolved = Vec::new();
    let mut resolved_set = HashSet::new();
    let mut allowed_agent_types: Option<Vec<String>> = None;

    for tool_spec in &agent.allowed_tools {
        let (tool_name, agent_types) = parse_tool_spec(tool_spec);

        // Extract allowedAgentTypes from "Agent(type1, type2)" specs
        if tool_name == ToolName::Agent.as_str()
            && let Some(types) = agent_types
        {
            allowed_agent_types = Some(types.into_iter().map(String::from).collect());
        }

        if available_set.contains(tool_name) {
            valid_tools.push(tool_spec.clone());
            if resolved_set.insert(tool_name) {
                resolved.push(tool_name.to_string());
            }
        } else {
            invalid_tools.push(tool_spec.clone());
        }
    }

    ResolvedAgentTools {
        has_wildcard: false,
        valid_tools,
        invalid_tools,
        resolved_tool_names: resolved,
        allowed_agent_types,
    }
}

// ── System prompt enhancement ──

/// Enhance an agent's system prompt with parent context information.
///
/// TS: enhanceSystemPromptWithEnvDetails() in runAgent.ts
pub fn enhance_agent_prompt(
    base_prompt: &str,
    parent_working_dir: &Path,
    agent_type: &str,
    is_background: bool,
) -> String {
    let mut enhanced = base_prompt.to_string();

    // Inject working directory context
    enhanced.push_str(&format!(
        "\n\nWorking directory: {}",
        parent_working_dir.display()
    ));

    // Background agent instructions
    if is_background {
        enhanced.push_str(
            "\n\nYou are running as a background agent. Write your final output \
             to the output file when complete. Do not ask questions — work \
             autonomously with available information.",
        );
    }

    // Agent type context
    if agent_type != "general-purpose" {
        enhanced.push_str(&format!("\n\nYou are a specialized '{agent_type}' agent."));
    }

    enhanced
}

// ── Background agent tracking ──

/// Tracks background agent execution.
///
/// TS: LocalAgentTask + ProgressTracker from agentToolUtils.ts
#[derive(Debug, Clone)]
pub struct BackgroundAgentTracker {
    pub agent_id: String,
    pub agent_type: String,
    pub status: BackgroundAgentStatus,
    pub tool_use_count: i64,
    pub total_tokens: i64,
    pub start_time_ms: i64,
}

/// Background agent execution status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundAgentStatus {
    Running,
    Completed,
    Failed { error: String },
}

/// Agent result summary.
///
/// TS: agentToolResultSchema — the structured result from a completed agent.
#[derive(Debug, Clone)]
pub struct AgentResultSummary {
    pub agent_id: String,
    pub agent_type: String,
    pub content: String,
    pub total_tool_use_count: i64,
    pub total_duration_ms: i64,
    pub total_tokens: i64,
}

/// Summarize agent results for the parent agent.
///
/// TS: content mapping in AgentTool call result
pub fn summarize_agent_result(
    agent_id: &str,
    agent_type: &str,
    result_text: &str,
    tool_use_count: i64,
    duration_ms: i64,
    tokens: i64,
) -> AgentResultSummary {
    AgentResultSummary {
        agent_id: agent_id.to_string(),
        agent_type: agent_type.to_string(),
        content: result_text.to_string(),
        total_tool_use_count: tool_use_count,
        total_duration_ms: duration_ms,
        total_tokens: tokens,
    }
}

/// Count tool uses across a set of messages (simplified).
///
/// TS: countToolUses()
pub fn count_tool_uses(messages: &[serde_json::Value]) -> i64 {
    let mut count: i64 = 0;
    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) == Some("assistant")
            && let Some(content) = msg.get("content").and_then(|c| c.as_array())
        {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    count += 1;
                }
            }
        }
    }
    count
}

// ── Agent discovery ──

/// Discover agents from multiple directories with deduplication.
///
/// TS: getAgentDefinitions() + getActiveAgentsFromList()
/// Sources are checked in priority order; later sources override earlier.
pub fn discover_agents(
    dirs: &[PathBuf],
    builtin_agents: &[AgentDefinition],
) -> Vec<AgentDefinition> {
    let mut agent_map: HashMap<String, AgentDefinition> = HashMap::new();

    // Built-in agents first (lowest priority)
    for agent in builtin_agents {
        agent_map.insert(agent.name.clone(), agent.clone());
    }

    // Directory agents override built-ins
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(dir)
            .max_depth(2)
            .follow_links(true)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "json" {
                continue;
            }

            match ext {
                "md" => {
                    if let Some(agent) = parse_md_agent(path) {
                        agent_map.insert(agent.name.clone(), agent);
                    }
                }
                "json" => {
                    for agent in parse_json_agents(path) {
                        agent_map.insert(agent.name.clone(), agent);
                    }
                }
                _ => {}
            }
        }
    }

    agent_map.into_values().collect()
}

/// Parse a markdown agent definition (frontmatter + body).
fn parse_md_agent(path: &Path) -> Option<AgentDefinition> {
    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return None;
    }

    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Check for YAML frontmatter
    if lines[0].trim() != "---" {
        return Some(AgentDefinition {
            agent_type: AgentTypeId::Custom(file_name.clone()),
            name: file_name,
            initial_prompt: Some(content),
            ..Default::default()
        });
    }

    // Find end of frontmatter
    let end_idx = lines.iter().skip(1).position(|l| l.trim() == "---")?;
    let end_idx = end_idx + 1; // Adjust for skip(1)

    // Parse frontmatter key-value pairs
    let mut fm: HashMap<String, String> = HashMap::new();
    for line in &lines[1..end_idx] {
        if let Some((key, value)) = line.split_once(':') {
            fm.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let body: String = lines[end_idx + 1..].join("\n").trim().to_string();
    let name = fm.get("name").cloned().unwrap_or(file_name);

    let parse_list = |key: &str| -> Vec<String> {
        fm.get(key)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    let parse_bool = |key: &str| -> bool {
        fm.get(key)
            .is_some_and(|v| v == "true" || v == "1" || v == "yes")
    };

    let isolation = fm
        .get("isolation")
        .and_then(|v| v.parse::<coco_types::AgentIsolation>().ok())
        .unwrap_or_default();

    let memory_scope = fm
        .get("memory")
        .and_then(|v| v.parse::<coco_types::MemoryScope>().ok());

    Some(AgentDefinition {
        agent_type: super::agent_spawn::resolve_agent_type(&name),
        name,
        description: fm.get("description").cloned(),
        initial_prompt: if body.is_empty() { None } else { Some(body) },
        allowed_tools: parse_list("tools"),
        disallowed_tools: parse_list("disallowedTools"),
        mcp_servers: parse_list("mcpServers"),
        required_mcp_servers: parse_list("requiredMcpServers"),
        max_turns: fm.get("maxTurns").and_then(|v| v.parse().ok()),
        model: fm.get("model").cloned(),
        color: fm.get("color").cloned(),
        skills: parse_list("skills"),
        background: parse_bool("background"),
        permission_mode: fm.get("permissionMode").cloned(),
        effort: fm.get("effort").cloned(),
        isolation,
        memory_scope,
        identity: fm.get("identity").cloned(),
        use_exact_tools: parse_bool("useExactTools"),
        omit_claude_md: parse_bool("omitClaudeMd"),
    })
}

/// Parse agents from a JSON file (record of agent_type -> definition).
///
/// TS: AgentsJsonSchema — z.record(z.string(), AgentJsonSchema)
fn parse_json_agents(path: &Path) -> Vec<AgentDefinition> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let map: HashMap<String, serde_json::Value> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let mut agents = Vec::new();
    for (agent_name, def) in map {
        let description = def
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let prompt = def.get("prompt").and_then(|v| v.as_str()).map(String::from);
        let model = def.get("model").and_then(|v| v.as_str()).map(String::from);
        let max_turns = def
            .get("maxTurns")
            .and_then(serde_json::Value::as_i64)
            .map(|v| v as i32);

        let parse_str_array = |key: &str| -> Vec<String> {
            def.get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        let get_bool = |key: &str| -> bool {
            def.get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        let get_str = |key: &str| -> Option<String> {
            def.get(key).and_then(|v| v.as_str()).map(String::from)
        };

        agents.push(AgentDefinition {
            agent_type: super::agent_spawn::resolve_agent_type(&agent_name),
            name: agent_name,
            description,
            initial_prompt: prompt,
            allowed_tools: parse_str_array("tools"),
            disallowed_tools: parse_str_array("disallowedTools"),
            mcp_servers: parse_str_array("mcpServers"),
            required_mcp_servers: parse_str_array("requiredMcpServers"),
            skills: parse_str_array("skills"),
            max_turns,
            model,
            color: get_str("color"),
            background: get_bool("background"),
            permission_mode: get_str("permissionMode"),
            effort: get_str("effort"),
            identity: get_str("identity"),
            use_exact_tools: get_bool("useExactTools"),
            omit_claude_md: get_bool("omitClaudeMd"),
            isolation: get_str("isolation")
                .and_then(|v| v.parse::<coco_types::AgentIsolation>().ok())
                .unwrap_or_default(),
            memory_scope: get_str("memory").and_then(|v| v.parse::<coco_types::MemoryScope>().ok()),
        });
    }

    agents
}

#[cfg(test)]
#[path = "agent_advanced.test.rs"]
mod tests;
