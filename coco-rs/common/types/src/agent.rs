use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

/// TS-parity built-in subagent types.
///
/// **Case is part of the contract.** TS treats `Explore` / `Plan` as PascalCase
/// (consumed by the case-sensitive one-shot set in `constants.ts`, by user
/// permission rules like `Agent(Explore)`, and by the
/// `tengu_agent_tool_selected` telemetry attribute). The remaining built-ins
/// (`general-purpose`, `statusline-setup`, `verification`, `claude-code-guide`)
/// stay lowercase. See `subagent-refactor-plan.md` § "Naming decision".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubagentType {
    GeneralPurpose,
    StatusLine,
    Explore,
    Plan,
    Verification,
    ClaudeCodeGuide,
}

impl SubagentType {
    /// Canonical TS string form. Output side must always use this exact case.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GeneralPurpose => "general-purpose",
            Self::StatusLine => "statusline-setup",
            Self::Explore => "Explore",
            Self::Plan => "Plan",
            Self::Verification => "verification",
            Self::ClaudeCodeGuide => "claude-code-guide",
        }
    }

    /// All built-in variants, in display order.
    pub const ALL: &'static [SubagentType] = &[
        Self::GeneralPurpose,
        Self::StatusLine,
        Self::Explore,
        Self::Plan,
        Self::Verification,
        Self::ClaudeCodeGuide,
    ];
}

impl fmt::Display for SubagentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SubagentType {
    type Err = String;

    /// Accept TS canonical case, plus lowercase aliases for `Explore`/`Plan`
    /// and underscore variants for the kebab-case names. Output is always
    /// canonical via `Display`/`as_str`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "general-purpose" | "general_purpose" => Ok(Self::GeneralPurpose),
            "statusline-setup" | "statusline_setup" => Ok(Self::StatusLine),
            "Explore" | "explore" => Ok(Self::Explore),
            "Plan" | "plan" => Ok(Self::Plan),
            "verification" => Ok(Self::Verification),
            "claude-code-guide" | "claude_code_guide" => Ok(Self::ClaudeCodeGuide),
            _ => Err(format!("unknown subagent type: {s}")),
        }
    }
}

impl Serialize for SubagentType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SubagentType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
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

// ── Agent Source ──

/// Where an agent definition came from. Drives precedence when the same
/// `agent_type` is defined in multiple places: later source wins.
///
/// TS: source field on `BaseAgentDefinition` (`loadAgentsDir.ts:105-165`),
/// resolution order in `getActiveAgentsFromList` (`loadAgentsDir.ts:193-221`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AgentSource {
    /// Bundled built-in agents.
    #[default]
    #[serde(rename = "built-in")]
    BuiltIn,
    /// Plugin contribution.
    #[serde(rename = "plugin")]
    Plugin,
    /// User-level agents (`~/.coco/agents/*.md`).
    #[serde(rename = "userSettings")]
    UserSettings,
    /// Project-level agents (`<project>/.coco/agents/*.md`).
    #[serde(rename = "projectSettings")]
    ProjectSettings,
    /// JSON/CLI flag supplied agents.
    #[serde(rename = "flagSettings")]
    FlagSettings,
    /// Managed/policy agents (highest priority).
    #[serde(rename = "policySettings")]
    PolicySettings,
}

impl AgentSource {
    /// Priority for conflict resolution. Higher wins.
    /// Mirrors TS `getActiveAgentsFromList` map-overwrite order.
    pub const fn priority(self) -> u8 {
        match self {
            Self::BuiltIn => 0,
            Self::Plugin => 1,
            Self::UserSettings => 2,
            Self::ProjectSettings => 3,
            Self::FlagSettings => 4,
            Self::PolicySettings => 5,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Plugin => "plugin",
            Self::UserSettings => "userSettings",
            Self::ProjectSettings => "projectSettings",
            Self::FlagSettings => "flagSettings",
            Self::PolicySettings => "policySettings",
        }
    }
}

impl fmt::Display for AgentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Agent Color ──

/// TS `AgentColorName`. Validated set; unknown values are dropped at parse
/// time with a warning so the runtime never sees an invalid color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentColorName {
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}

impl AgentColorName {
    pub const ALL: &'static [AgentColorName] = &[
        Self::Red,
        Self::Blue,
        Self::Green,
        Self::Yellow,
        Self::Purple,
        Self::Orange,
        Self::Pink,
        Self::Cyan,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Purple => "purple",
            Self::Orange => "orange",
            Self::Pink => "pink",
            Self::Cyan => "cyan",
        }
    }
}

impl fmt::Display for AgentColorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentColorName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "red" => Ok(Self::Red),
            "blue" => Ok(Self::Blue),
            "green" => Ok(Self::Green),
            "yellow" => Ok(Self::Yellow),
            "purple" => Ok(Self::Purple),
            "orange" => Ok(Self::Orange),
            "pink" => Ok(Self::Pink),
            "cyan" => Ok(Self::Cyan),
            other => Err(format!("unknown agent color: {other}")),
        }
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
/// TS: AgentDefinition in loadAgentsDir.ts (`BaseAgentDefinition` +
/// `BuiltInAgentDefinition` / `CustomAgentDefinition` / `PluginAgentDefinition`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Agent type identity (TS `agentType`).
    pub agent_type: AgentTypeId,

    /// Human-readable display name.
    #[serde(default)]
    pub name: String,

    /// User-facing summary shown in the AgentTool prompt list.
    /// TS: `whenToUse` in `BaseAgentDefinition`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,

    /// Free-form description (kept for back-compat; loaders should mirror it
    /// into `when_to_use` when the source has no separate `whenToUse`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Where the definition came from. Drives precedence when the same
    /// `agent_type` is defined in multiple sources.
    #[serde(default)]
    pub source: AgentSource,

    /// Original markdown filename, for diagnostics and `/agents show`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Source directory (e.g. `<project>/.coco/agents`), for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,

    /// System prompt body (TS markdown body / JSON `prompt`). Distinct from
    /// `initial_prompt`, which is a first-turn user-message prefix.
    ///
    /// Built-in agents leave this empty and provide a renderer that produces
    /// the prompt at spawn time using the parent context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

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

    /// Starting prompt prefix for the first user turn (TS `initialPrompt`).
    /// **Not** the system prompt — see `system_prompt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,

    /// Maximum turns before the agent should stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,

    /// Tools this agent is not allowed to use (deny-list).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,

    /// Tools this agent is explicitly allowed to use (allow-list).
    /// Empty means "use the default filtered set".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,

    /// System prompt / identity override (legacy — prefer `system_prompt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,

    /// Validated UI color for this agent type (TS `color`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<AgentColorName>,

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

    /// Short reminder re-injected at every user turn.
    /// TS: `criticalSystemReminder_EXPERIMENTAL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_system_reminder: Option<String>,
}

impl Default for AgentDefinition {
    fn default() -> Self {
        Self {
            agent_type: AgentTypeId::Custom("default".into()),
            name: String::new(),
            when_to_use: None,
            description: None,
            source: AgentSource::BuiltIn,
            filename: None,
            base_dir: None,
            system_prompt: None,
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
            critical_system_reminder: None,
        }
    }
}

#[cfg(test)]
#[path = "agent.test.rs"]
mod tests;
