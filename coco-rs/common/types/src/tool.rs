use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

use crate::Message;

/// Prefix for MCP tool qualified names: `mcp__<server>__<tool>`.
pub const MCP_TOOL_PREFIX: &str = "mcp__";

/// Separator between server and tool in MCP qualified names.
pub const MCP_TOOL_SEPARATOR: &str = "__";

/// Branch prefix for agent worktrees created by `EnterWorktree`.
pub const AGENT_WORKTREE_BRANCH_PREFIX: &str = "agent/task-";

/// All 41 built-in tool names.
/// Copy + const fn as_str() — zero-cost identity for builtins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ToolName {
    // File I/O (7)
    Bash,
    Read,
    Write,
    Edit,
    Glob,
    Grep,
    NotebookEdit,
    // Web (2)
    WebFetch,
    WebSearch,
    // Agent & Team (5)
    Agent,
    Skill,
    SendMessage,
    TeamCreate,
    TeamDelete,
    // Task Management (7)
    TaskCreate,
    TaskGet,
    TaskList,
    TaskUpdate,
    TaskStop,
    TaskOutput,
    TodoWrite,
    // Plan & Worktree (4)
    EnterPlanMode,
    ExitPlanMode,
    EnterWorktree,
    ExitWorktree,
    // Utility (5)
    AskUserQuestion,
    ToolSearch,
    Config,
    Brief,
    #[serde(rename = "LSP")]
    Lsp,
    // MCP management (3)
    McpAuth,
    ListMcpResources,
    ReadMcpResource,
    // Scheduling (4)
    CronCreate,
    CronDelete,
    CronList,
    RemoteTrigger,
    // Shell (2)
    PowerShell,
    #[serde(rename = "REPL")]
    Repl,
    // Internal/SDK (2)
    Sleep,
    SyntheticOutput,
}

impl ToolName {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Bash => "Bash",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::Edit => "Edit",
            Self::Glob => "Glob",
            Self::Grep => "Grep",
            Self::NotebookEdit => "NotebookEdit",
            Self::WebFetch => "WebFetch",
            Self::WebSearch => "WebSearch",
            Self::Agent => "Agent",
            Self::Skill => "Skill",
            Self::SendMessage => "SendMessage",
            Self::TeamCreate => "TeamCreate",
            Self::TeamDelete => "TeamDelete",
            Self::TaskCreate => "TaskCreate",
            Self::TaskGet => "TaskGet",
            Self::TaskList => "TaskList",
            Self::TaskUpdate => "TaskUpdate",
            Self::TaskStop => "TaskStop",
            Self::TaskOutput => "TaskOutput",
            Self::TodoWrite => "TodoWrite",
            Self::EnterPlanMode => "EnterPlanMode",
            Self::ExitPlanMode => "ExitPlanMode",
            Self::EnterWorktree => "EnterWorktree",
            Self::ExitWorktree => "ExitWorktree",
            Self::AskUserQuestion => "AskUserQuestion",
            Self::ToolSearch => "ToolSearch",
            Self::Config => "Config",
            Self::Brief => "Brief",
            Self::Lsp => "LSP",
            Self::McpAuth => "McpAuth",
            Self::ListMcpResources => "ListMcpResources",
            Self::ReadMcpResource => "ReadMcpResource",
            Self::CronCreate => "CronCreate",
            Self::CronDelete => "CronDelete",
            Self::CronList => "CronList",
            Self::RemoteTrigger => "RemoteTrigger",
            Self::PowerShell => "PowerShell",
            Self::Repl => "REPL",
            Self::Sleep => "Sleep",
            Self::SyntheticOutput => "SyntheticOutput",
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Bash" => Ok(Self::Bash),
            "Read" => Ok(Self::Read),
            "Write" => Ok(Self::Write),
            "Edit" => Ok(Self::Edit),
            "Glob" => Ok(Self::Glob),
            "Grep" => Ok(Self::Grep),
            "NotebookEdit" => Ok(Self::NotebookEdit),
            "WebFetch" => Ok(Self::WebFetch),
            "WebSearch" => Ok(Self::WebSearch),
            "Agent" => Ok(Self::Agent),
            "Skill" => Ok(Self::Skill),
            "SendMessage" => Ok(Self::SendMessage),
            "TeamCreate" => Ok(Self::TeamCreate),
            "TeamDelete" => Ok(Self::TeamDelete),
            "TaskCreate" => Ok(Self::TaskCreate),
            "TaskGet" => Ok(Self::TaskGet),
            "TaskList" => Ok(Self::TaskList),
            "TaskUpdate" => Ok(Self::TaskUpdate),
            "TaskStop" => Ok(Self::TaskStop),
            "TaskOutput" => Ok(Self::TaskOutput),
            "TodoWrite" => Ok(Self::TodoWrite),
            "EnterPlanMode" => Ok(Self::EnterPlanMode),
            "ExitPlanMode" => Ok(Self::ExitPlanMode),
            "EnterWorktree" => Ok(Self::EnterWorktree),
            "ExitWorktree" => Ok(Self::ExitWorktree),
            "AskUserQuestion" => Ok(Self::AskUserQuestion),
            "ToolSearch" => Ok(Self::ToolSearch),
            "Config" => Ok(Self::Config),
            "Brief" => Ok(Self::Brief),
            "LSP" => Ok(Self::Lsp),
            "McpAuth" => Ok(Self::McpAuth),
            "ListMcpResources" => Ok(Self::ListMcpResources),
            "ReadMcpResource" => Ok(Self::ReadMcpResource),
            "CronCreate" => Ok(Self::CronCreate),
            "CronDelete" => Ok(Self::CronDelete),
            "CronList" => Ok(Self::CronList),
            "RemoteTrigger" => Ok(Self::RemoteTrigger),
            "PowerShell" => Ok(Self::PowerShell),
            "REPL" => Ok(Self::Repl),
            "Sleep" => Ok(Self::Sleep),
            "SyntheticOutput" => Ok(Self::SyntheticOutput),
            _ => Err(format!("unknown tool name: {s}")),
        }
    }
}

/// Tool identity — type-safe for all tool kinds.
///
/// Three distinct concepts:
///   ToolId      = identity ("who am I")         → this enum
///   ToolName    = built-in tools only (Copy)     → inner enum, 41 variants
///   ToolPattern = permission match expression    → String ("Bash(git *)", "mcp__slack__*")
///
/// Serde: serializes/deserializes as a FLAT STRING via Display/FromStr.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolId {
    /// Built-in tool (41 variants, Copy, const fn as_str()).
    Builtin(ToolName),
    /// MCP tool: structured server + tool name.
    /// Wire format: "mcp__<server>__<tool>"
    Mcp { server: String, tool: String },
    /// Plugin/custom tool (future extensibility).
    Custom(String),
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(name) => f.write_str(name.as_str()),
            Self::Mcp { server, tool } => {
                write!(f, "{MCP_TOOL_PREFIX}{server}{MCP_TOOL_SEPARATOR}{tool}")
            }
            Self::Custom(name) => f.write_str(name),
        }
    }
}

/// Parses wire-format string. "mcp__server__tool" → Mcp, known → Builtin, else → Custom.
impl FromStr for ToolId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix(MCP_TOOL_PREFIX)
            && let Some((server, tool)) = rest.split_once(MCP_TOOL_SEPARATOR)
        {
            return Ok(Self::Mcp {
                server: server.into(),
                tool: tool.into(),
            });
        }
        Ok(ToolName::from_str(s)
            .map(Self::Builtin)
            .unwrap_or_else(|_| Self::Custom(s.into())))
    }
}

impl Serialize for ToolId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        // Infallible — unwrap is safe
        Ok(s.parse().unwrap())
    }
}

impl From<ToolName> for ToolId {
    fn from(name: ToolName) -> Self {
        Self::Builtin(name)
    }
}

impl ToolId {
    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin(_))
    }

    pub fn is_mcp(&self) -> bool {
        matches!(self, Self::Mcp { .. })
    }

    pub fn mcp_server(&self) -> Option<&str> {
        match self {
            Self::Mcp { server, .. } => Some(server),
            _ => None,
        }
    }
}

/// JSON Schema properties for tool input. Type is always "object" (not stored).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInputSchema {
    pub properties: HashMap<String, serde_json::Value>,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult<T> {
    pub data: T,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_messages: Vec<Message>,
}

/// Progress report during tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgress {
    pub tool_use_id: String,
    pub data: serde_json::Value,
}

#[cfg(test)]
#[path = "tool.test.rs"]
mod tests;
