use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use serde::Deserialize;
use serde::Serialize;

/// Declarative definition of a subagent type.
///
/// Each definition specifies the agent's name, description, allowed/disallowed
/// tools, and optional model and turn limit overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentDefinition {
    /// Unique name for this agent type (e.g. "bash", "explore").
    pub name: String,

    /// Human-readable description of the agent's purpose.
    pub description: String,

    /// Agent type identifier used for spawning.
    pub agent_type: String,

    /// Allowed tools (empty means all tools are available).
    #[serde(default)]
    pub tools: Vec<String>,

    /// Tools explicitly denied to this agent.
    #[serde(default)]
    pub disallowed_tools: Vec<String>,

    /// Model selection identity for this agent type.
    ///
    /// Determines how the model is resolved:
    /// - `Role(ModelRole)`: Use the model configured for that role
    /// - `Spec(ModelSpec)`: Use a specific provider/model
    /// - `Inherit`: Use the parent agent's model
    /// - `None`: Fall back to parent model (same as Inherit)
    #[serde(default)]
    pub identity: Option<ExecutionIdentity>,

    /// Override the maximum number of turns for this agent.
    #[serde(default)]
    pub max_turns: Option<i32>,

    /// Override the permission mode for this subagent.
    ///
    /// When set, the subagent uses this permission mode instead of
    /// inheriting the parent's mode. For example, a "guide" agent
    /// that only reads docs might use `DontAsk` to auto-deny unknown
    /// operations, while a "bash" agent uses `Default`.
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,

    /// Whether to fork the parent conversation context to this agent.
    /// Only `general` uses this (gets conversation history).
    #[serde(default)]
    pub fork_context: bool,

    /// Display color for TUI (e.g., "cyan", "blue", "green", "orange").
    #[serde(default)]
    pub color: Option<String>,

    /// Critical reminder injected at the start of the agent's prompt.
    /// Used for read-only enforcement in explore/plan/guide agents.
    #[serde(default)]
    pub critical_reminder: Option<String>,

    /// Where this definition originates from.
    #[serde(default)]
    pub source: AgentSource,

    /// Skills to load for this agent (by name).
    #[serde(default)]
    pub skills: Vec<String>,

    /// Default background mode for this agent.
    ///
    /// When `true`, the agent runs in the background by default unless
    /// the spawn input explicitly overrides it.
    #[serde(default)]
    pub background: bool,

    /// Memory scope for persistent agent memory.
    ///
    /// When set, the agent gets a persistent memory directory and its
    /// `MEMORY.md` (first 200 lines) is injected into the prompt.
    #[serde(default)]
    pub memory: Option<MemoryScope>,

    /// Hook definitions scoped to this agent's lifecycle.
    ///
    /// These hooks are registered when the agent starts and unregistered
    /// when it completes. A `Stop` event in agent hooks is remapped to
    /// `SubagentStop`.
    #[serde(default)]
    pub hooks: Option<Vec<AgentHookDefinition>>,

    /// MCP server references required by this agent.
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServerRef>>,

    /// Isolation mode for this agent's execution environment.
    #[serde(default)]
    pub isolation: Option<IsolationMode>,

    /// Whether the markdown body (`critical_reminder`) is the full system prompt.
    ///
    /// When `true` and `critical_reminder` is set, the body replaces the entire
    /// generated system prompt instead of being prepended to the user prompt.
    #[serde(default)]
    pub use_custom_prompt: bool,
}

/// Where an agent definition originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AgentSource {
    #[default]
    BuiltIn,
    UserSettings,
    ProjectSettings,
    Plugin,
    /// Agent provided by an SDK client at session initialization.
    Sdk,
    CliFlag,
}

impl AgentSource {
    /// Returns the priority of this source (higher = takes precedence).
    pub fn priority(self) -> u8 {
        match self {
            AgentSource::BuiltIn => 0,
            AgentSource::Plugin => 1,
            AgentSource::UserSettings => 2,
            AgentSource::ProjectSettings => 3,
            AgentSource::Sdk => 4,
            AgentSource::CliFlag => 5,
        }
    }
}

/// Memory scope for persistent agent memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// User-level memory: `~/.cocode/agent-memory/{agent_type}/`
    User,
    /// Project-level memory: `.cocode/agent-memory/{agent_type}/`
    Project,
    /// Local (gitignored) memory: `.cocode/agent-memory-local/{agent_type}/`
    Local,
}

impl MemoryScope {
    /// Resolve the memory directory for an agent with this scope.
    pub fn resolve_dir(
        &self,
        cocode_home: &std::path::Path,
        working_dir: &std::path::Path,
        agent_type: &str,
    ) -> std::path::PathBuf {
        match self {
            Self::User => cocode_home.join("agent-memory").join(agent_type),
            Self::Project => working_dir
                .join(".cocode")
                .join("agent-memory")
                .join(agent_type),
            Self::Local => working_dir
                .join(".cocode")
                .join("agent-memory-local")
                .join(agent_type),
        }
    }
}

/// A lightweight hook definition scoped to an agent.
///
/// Similar to `HookDefinition` but serialized from agent frontmatter.
/// The `Stop` event type is remapped to `SubagentStop` when registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHookDefinition {
    /// The event type this hook triggers on.
    pub event: String,
    /// Optional matcher pattern (e.g., tool name).
    #[serde(default)]
    pub matcher: Option<String>,
    /// The command to execute.
    pub command: String,
    /// Optional timeout in seconds.
    #[serde(default)]
    pub timeout: Option<u32>,
}

/// Reference to an MCP server required by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRef {
    /// The MCP server name (must match a configured server).
    pub name: String,
    /// Optional transport override (e.g., "stdio", "sse").
    #[serde(default)]
    pub transport: Option<String>,
}

/// Isolation mode for agent execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IsolationMode {
    /// No isolation (default) — agent shares parent's working directory.
    None,
    /// Git worktree isolation — agent runs in a detached worktree.
    Worktree,
}

impl AgentDefinition {
    /// Merge this definition with a higher-priority override.
    ///
    /// Follows CC's merging semantics:
    /// - Scalar fields (identity, max_turns, color, background, etc.): `other` overrides `self`
    ///   only if `other` has a non-default value
    /// - Array fields (tools, disallowed_tools, skills): union (append without duplicates)
    /// - Optional fields: `other` takes precedence when `Some`
    /// - Hooks: merged via union
    pub fn merge_with(&self, other: &AgentDefinition) -> AgentDefinition {
        let mut merged = self.clone();

        // Scalar overrides: other wins if non-default
        if !other.description.is_empty() {
            merged.description = other.description.clone();
        }
        if other.identity.is_some() {
            merged.identity = other.identity.clone();
        }
        if other.max_turns.is_some() {
            merged.max_turns = other.max_turns;
        }
        if other.permission_mode.is_some() {
            merged.permission_mode = other.permission_mode;
        }
        if other.fork_context {
            merged.fork_context = true;
        }
        if other.color.is_some() {
            merged.color = other.color.clone();
        }
        if other.critical_reminder.is_some() {
            merged.critical_reminder = other.critical_reminder.clone();
        }
        if other.background {
            merged.background = true;
        }
        if other.memory.is_some() {
            merged.memory = other.memory;
        }
        if other.mcp_servers.is_some() {
            merged.mcp_servers = other.mcp_servers.clone();
        }
        if other.isolation.is_some() {
            merged.isolation = other.isolation;
        }
        if other.use_custom_prompt {
            merged.use_custom_prompt = true;
        }

        // Array union: append without duplicates
        for tool in &other.tools {
            if !merged.tools.contains(tool) {
                merged.tools.push(tool.clone());
            }
        }
        for tool in &other.disallowed_tools {
            if !merged.disallowed_tools.contains(tool) {
                merged.disallowed_tools.push(tool.clone());
            }
        }
        for skill in &other.skills {
            if !merged.skills.contains(skill) {
                merged.skills.push(skill.clone());
            }
        }

        // Hooks: merge via union
        match (&merged.hooks, &other.hooks) {
            (Some(base), Some(extra)) => {
                let mut combined = base.clone();
                combined.extend(extra.iter().cloned());
                merged.hooks = Some(combined);
            }
            (None, Some(hooks)) => {
                merged.hooks = Some(hooks.clone());
            }
            _ => {}
        }

        // Source: take the higher-priority source
        if other.source.priority() > merged.source.priority() {
            merged.source = other.source;
        }

        merged
    }
}

#[cfg(test)]
#[path = "definition.test.rs"]
mod tests;
