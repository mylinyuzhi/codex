//! Aggregate conversation context.
//!
//! Combines environment info, budget, tool state, memory, and configuration
//! into a single context value used by the prompt builder.

use cocode_protocol::CompactConfig;
use cocode_protocol::PermissionMode;
use cocode_protocol::SessionMemoryConfig;
use cocode_protocol::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;

/// Output style configuration that affects system prompt generation.
///
/// When active, modifies the system prompt by:
/// - Stripping the "Communication Style" section from identity
/// - Conditionally removing coding-specific sections (ToolPolicy, GitWorkflow, TaskManagement)
/// - Appending the style's custom instructions at the end of the prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStylePromptConfig {
    /// The style name (for display purposes).
    pub name: String,
    /// The style content/instructions to append to the prompt.
    pub content: String,
    /// Whether to keep coding-specific sections (ToolPolicy, GitWorkflow, TaskManagement).
    /// Built-in styles default to true; custom styles default to false.
    pub keep_coding_instructions: bool,
}

use crate::budget::ContextBudget;
use crate::environment::EnvironmentInfo;

/// A memory file loaded into context (CLAUDE.md, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFile {
    /// File path (relative or display name).
    pub path: String,
    /// File content.
    pub content: String,
    /// Priority for ordering (lower = higher priority).
    pub priority: i32,
}

/// Content injected into the system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInjection {
    /// Label for this injection.
    pub label: String,
    /// Content to inject.
    pub content: String,
    /// Where to inject this content.
    pub position: InjectionPosition,
}

/// Position for injected content in the system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionPosition {
    /// Before tool definitions section.
    BeforeTools,
    /// After tool definitions section.
    AfterTools,
    /// At the end of the prompt.
    EndOfPrompt,
}

/// Type of subagent for specialized prompt generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentType {
    /// Codebase exploration subagent.
    Explore,
    /// Implementation planning subagent.
    Plan,
}

impl SubagentType {
    /// Get the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            SubagentType::Explore => "explore",
            SubagentType::Plan => "plan",
        }
    }
}

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Aggregate conversation context for prompt generation.
#[derive(Debug, Clone)]
pub struct ConversationContext {
    /// Runtime environment information.
    pub environment: EnvironmentInfo,
    /// Token budget tracker.
    pub budget: ContextBudget,
    /// Available tool names.
    pub tool_names: Vec<String>,
    /// Connected MCP server names.
    pub mcp_server_names: Vec<String>,
    /// Loaded memory files.
    pub memory_files: Vec<MemoryFile>,
    /// Prompt injections.
    pub injections: Vec<ContextInjection>,
    /// Permission mode for the session.
    pub permission_mode: PermissionMode,
    /// Thinking/reasoning configuration.
    pub thinking_level: Option<ThinkingLevel>,
    /// Compaction configuration.
    pub compact_config: CompactConfig,
    /// Session memory configuration.
    pub session_memory_config: SessionMemoryConfig,
    /// Subagent type (if this is a subagent).
    pub subagent_type: Option<SubagentType>,
    /// Path to the conversation transcript file.
    pub transcript_path: Option<std::path::PathBuf>,
    /// Output style configuration for prompt modification.
    pub output_style: Option<OutputStylePromptConfig>,
}

impl ConversationContext {
    /// Create a builder for constructing conversation context.
    pub fn builder() -> ConversationContextBuilder {
        ConversationContextBuilder::default()
    }

    /// Check if any MCP servers are connected.
    pub fn has_mcp_servers(&self) -> bool {
        !self.mcp_server_names.is_empty()
    }

    /// Check if any tools are available.
    pub fn has_tools(&self) -> bool {
        !self.tool_names.is_empty()
    }

    /// Check if this is a subagent context.
    pub fn is_subagent(&self) -> bool {
        self.subagent_type.is_some()
    }
}

/// Builder for [`ConversationContext`].
#[derive(Debug, Default)]
pub struct ConversationContextBuilder {
    environment: Option<EnvironmentInfo>,
    budget: Option<ContextBudget>,
    tool_names: Vec<String>,
    mcp_server_names: Vec<String>,
    memory_files: Vec<MemoryFile>,
    injections: Vec<ContextInjection>,
    permission_mode: PermissionMode,
    thinking_level: Option<ThinkingLevel>,
    compact_config: CompactConfig,
    session_memory_config: SessionMemoryConfig,
    subagent_type: Option<SubagentType>,
    transcript_path: Option<std::path::PathBuf>,
    output_style: Option<OutputStylePromptConfig>,
}

impl ConversationContextBuilder {
    pub fn environment(mut self, env: EnvironmentInfo) -> Self {
        self.environment = Some(env);
        self
    }

    pub fn budget(mut self, budget: ContextBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn tool_names(mut self, names: Vec<String>) -> Self {
        self.tool_names = names;
        self
    }

    pub fn mcp_server_names(mut self, names: Vec<String>) -> Self {
        self.mcp_server_names = names;
        self
    }

    pub fn memory_files(mut self, files: Vec<MemoryFile>) -> Self {
        self.memory_files = files;
        self
    }

    pub fn injections(mut self, injections: Vec<ContextInjection>) -> Self {
        self.injections = injections;
        self
    }

    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    pub fn thinking_level(mut self, config: ThinkingLevel) -> Self {
        self.thinking_level = Some(config);
        self
    }

    pub fn compact_config(mut self, config: CompactConfig) -> Self {
        self.compact_config = config;
        self
    }

    pub fn session_memory_config(mut self, config: SessionMemoryConfig) -> Self {
        self.session_memory_config = config;
        self
    }

    pub fn subagent_type(mut self, agent_type: SubagentType) -> Self {
        self.subagent_type = Some(agent_type);
        self
    }

    pub fn transcript_path(mut self, path: std::path::PathBuf) -> Self {
        self.transcript_path = Some(path);
        self
    }

    pub fn output_style(mut self, config: OutputStylePromptConfig) -> Self {
        self.output_style = Some(config);
        self
    }

    /// Build the [`ConversationContext`].
    ///
    /// Returns `Err` if required fields are missing.
    pub fn build(self) -> crate::error::Result<ConversationContext> {
        let environment = self.environment.ok_or_else(|| {
            crate::error::context_error::BuildSnafu {
                message: "environment is required",
            }
            .build()
        })?;

        let budget = self.budget.unwrap_or_else(|| {
            ContextBudget::new(environment.context_window, environment.max_output_tokens)
        });

        Ok(ConversationContext {
            environment,
            budget,
            tool_names: self.tool_names,
            mcp_server_names: self.mcp_server_names,
            memory_files: self.memory_files,
            injections: self.injections,
            permission_mode: self.permission_mode,
            thinking_level: self.thinking_level,
            compact_config: self.compact_config,
            session_memory_config: self.session_memory_config,
            subagent_type: self.subagent_type,
            transcript_path: self.transcript_path,
            output_style: self.output_style,
        })
    }
}

#[cfg(test)]
#[path = "conversation_context.test.rs"]
mod tests;
