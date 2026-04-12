use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

/// 7 built-in subagent types (matches TS AgentTool loadAgentsDir.ts).
/// Copy + const fn as_str() — same pattern as ToolName.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentType {
    Explore,
    Plan,
    Review,
    StatusLine,
    ClaudeCodeGuide,
    Fork,
    HookAgent,
}

impl SubagentType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::Plan => "plan",
            Self::Review => "review",
            Self::StatusLine => "statusline-setup",
            Self::ClaudeCodeGuide => "claude-code-guide",
            Self::Fork => "fork",
            Self::HookAgent => "hook-agent",
        }
    }
}

impl fmt::Display for SubagentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SubagentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "explore" => Ok(Self::Explore),
            "plan" => Ok(Self::Plan),
            "review" => Ok(Self::Review),
            "statusline-setup" | "statusline_setup" => Ok(Self::StatusLine),
            "claude-code-guide" | "claude_code_guide" => Ok(Self::ClaudeCodeGuide),
            "fork" => Ok(Self::Fork),
            "hook-agent" | "hook_agent" => Ok(Self::HookAgent),
            _ => Err(format!("unknown subagent type: {s}")),
        }
    }
}

/// Agent identity — same pattern as ToolId.
/// Serde as flat string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentTypeId {
    Builtin(SubagentType),
    Custom(String),
}

impl fmt::Display for AgentTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(t) => f.write_str(t.as_str()),
            Self::Custom(name) => f.write_str(name),
        }
    }
}

impl FromStr for AgentTypeId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SubagentType::from_str(s)
            .map(Self::Builtin)
            .unwrap_or_else(|_| Self::Custom(s.into())))
    }
}

impl Serialize for AgentTypeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AgentTypeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        // Infallible parse — AgentTypeId::from_str always succeeds
        // (unknown strings become Custom variants).
        Ok(s.parse().expect("AgentTypeId::from_str is Infallible"))
    }
}

impl From<SubagentType> for AgentTypeId {
    fn from(t: SubagentType) -> Self {
        Self::Builtin(t)
    }
}

// ── Agent Isolation ──

/// Isolation mode for a subagent's execution environment.
///
/// TS: AgentIsolation in loadAgentsDir.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentIsolation {
    /// No isolation — agent shares the parent's working directory.
    #[default]
    None,
    /// Git worktree — agent gets an isolated worktree branch.
    Worktree,
    /// Remote execution via Claude Code Remote (CCR).
    Remote,
}

impl fmt::Display for AgentIsolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Worktree => f.write_str("worktree"),
            Self::Remote => f.write_str("remote"),
        }
    }
}

impl FromStr for AgentIsolation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "worktree" => Ok(Self::Worktree),
            "remote" => Ok(Self::Remote),
            _ => Err(format!("unknown agent isolation mode: {s}")),
        }
    }
}

// ── Memory Scope ──

/// Scope for agent memory persistence.
///
/// TS: MemoryScope — controls where MEMORY.md is stored/read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    /// User-global memory (~/.coco/MEMORY.md).
    User,
    /// Project-scoped memory (project root MEMORY.md).
    #[default]
    Project,
    /// Local-only memory (not persisted across sessions).
    Local,
}

impl fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Project => f.write_str("project"),
            Self::Local => f.write_str("local"),
        }
    }
}

impl FromStr for MemoryScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            "local" => Ok(Self::Local),
            _ => Err(format!("unknown memory scope: {s}")),
        }
    }
}

// ── Model Inheritance ──

/// Where a model setting was resolved from (for inheritance debugging).
///
/// Precedence: Param > Definition > Parent (highest to lowest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    /// Explicitly passed as a spawn parameter.
    Param,
    /// Defined in the agent definition file.
    Definition,
    /// Inherited from the parent agent.
    Parent,
}

impl fmt::Display for ModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param => f.write_str("param"),
            Self::Definition => f.write_str("definition"),
            Self::Parent => f.write_str("parent"),
        }
    }
}

/// Tracks how a model was resolved through the inheritance chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInheritance {
    /// The resolved model identifier.
    pub model: String,
    /// Where the model value was resolved from.
    pub source: ModelSource,
}

// ── Agent Definition ──

/// Complete agent definition — the declarative spec for a subagent.
///
/// TS: AgentDefinition in loadAgentsDir.ts / subagent.ts
/// Combines identity, capabilities, and configuration overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Agent type identity.
    pub agent_type: AgentTypeId,

    /// Human-readable display name.
    #[serde(default)]
    pub name: String,

    /// Agent description (shown in tool listings).
    /// TS: `whenToUse` / `description`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Thinking/effort level override for this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,

    /// Cache-identical tool schema prefixes for stable cache keys.
    #[serde(default)]
    pub use_exact_tools: bool,

    /// Model override for this agent. Use `"inherit"` for parent's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Isolation mode for the agent's execution environment.
    #[serde(default)]
    pub isolation: AgentIsolation,

    /// Memory persistence scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<MemoryScope>,

    /// Per-agent MCP server names to connect.
    /// TS: `mcpServers`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,

    /// Starting prompt/instructions for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,

    /// Maximum turns before the agent should stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,

    /// Tools this agent is not allowed to use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,

    /// Tools this agent is explicitly allowed to use.
    /// Supports `"Agent(type1, type2)"` syntax for restricting subagent types.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,

    /// System prompt / identity override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,

    // ── Fields added for TS alignment (loadAgentsDir.ts) ──
    /// UI color for this agent type.
    /// TS: `color: AgentColorName`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Skill names to preload when this agent starts.
    /// TS: `skills: string[]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,

    /// Whether this agent always runs in the background.
    /// TS: `background: boolean`
    #[serde(default)]
    pub background: bool,

    /// Permission mode override for this agent.
    /// TS: `permissionMode: PermissionMode`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,

    /// MCP server name patterns required (agent disabled if not configured).
    /// Separate from `mcp_servers` which are servers to connect.
    /// TS: `requiredMcpServers: string[]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_mcp_servers: Vec<String>,

    /// Omit CLAUDE.md context for read-only agents.
    /// TS: `omitClaudeMd: boolean`
    #[serde(default)]
    pub omit_claude_md: bool,
}

impl Default for AgentDefinition {
    fn default() -> Self {
        Self {
            agent_type: AgentTypeId::Custom("default".into()),
            name: String::new(),
            description: None,
            effort: None,
            use_exact_tools: false,
            model: None,
            isolation: AgentIsolation::None,
            memory_scope: None,
            mcp_servers: Vec::new(),
            initial_prompt: None,
            max_turns: None,
            disallowed_tools: Vec::new(),
            allowed_tools: Vec::new(),
            identity: None,
            color: None,
            skills: Vec::new(),
            background: false,
            permission_mode: None,
            required_mcp_servers: Vec::new(),
            omit_claude_md: false,
        }
    }
}

#[cfg(test)]
#[path = "agent.test.rs"]
mod tests;
