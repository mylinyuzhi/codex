//! cocode-tools-api - Tool API surface for the agent system.
//!
//! This crate provides the tool system API:
//! - Tool trait with 5-stage pipeline and input-dependent concurrency
//! - Tool registry (built-in + MCP)
//! - Streaming tool executor
//! - ToolContext and sub-structs
//! - Error types
//!
//! Built-in tool implementations are provided by `cocode-tools`.

pub mod context;
pub mod error;
pub mod executor;
pub mod executor_builder;
pub mod executor_hooks;
pub mod file_tracker;
pub mod mcp_tool;
pub mod permission;
pub mod question;
pub mod registry;
pub mod result_persistence;
pub mod sensitive_files;
pub mod spawn_agent;
pub mod tool;

// Re-export from cocode-policy (permission types moved there)
pub use cocode_policy::ApprovalStore;
pub use cocode_policy::PermissionRule;
pub use cocode_policy::PermissionRuleEvaluator;
pub use cocode_policy::RuleAction;

// Re-export main types at crate root
pub use context::AgentContext;
pub use context::SessionPaths;
pub use context::ToolCallIdentity;
pub use context::ToolChannels;
pub use context::ToolContext;
pub use context::ToolContextBuilder;
pub use context::ToolEnvironment;
pub use context::ToolServices;
pub use context::ToolSharedState;
pub use error::Result;
pub use error::ToolError;
pub use executor::ExecutorConfig;
pub use executor::StreamingToolExecutor;
pub use executor::ToolExecutionResult;
pub use file_tracker::FileReadState;
pub use file_tracker::FileTracker;
pub use mcp_tool::McpToolWrapper;
pub use permission::InvokedSkill;
pub use permission::PermissionRequester;
pub use question::QuestionResponder;
pub use registry::McpToolInfo;
pub use registry::ToolRegistry;
pub use spawn_agent::AgentCancelTokens;
pub use spawn_agent::KilledAgents;
pub use spawn_agent::ModelCallFn;
pub use spawn_agent::ModelCallInput;
pub use spawn_agent::ModelCallResult;
pub use spawn_agent::SpawnAgentFn;
pub use spawn_agent::SpawnAgentInput;
pub use spawn_agent::SpawnAgentResult;
pub use tool::Tool;
pub use tool::ToolOutputExt;

// Re-export commonly used types from dependencies
pub use cocode_inference::ToolCall;
pub use cocode_protocol::AbortReason;
pub use cocode_protocol::ApprovalDecision;
pub use cocode_protocol::ConcurrencySafety;
pub use cocode_protocol::ContextModifier;
pub use cocode_protocol::PermissionMode;
pub use cocode_protocol::PermissionResult;
pub use cocode_protocol::ToolOutput;
pub use cocode_protocol::ToolResultContent;
pub use cocode_protocol::ValidationResult;

/// A tool definition for the API.
///
/// This is the Vercel AI SDK v4 function tool type (`LanguageModelFunctionTool`).
/// Fields: `name`, `description: Option<String>`, `input_schema: JSONSchema`,
/// `input_examples`, `strict`, `provider_options`.
pub type ToolDefinition = cocode_inference::LanguageModelFunctionTool;
