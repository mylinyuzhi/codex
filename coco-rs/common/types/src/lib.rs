//! Foundation types shared across all coco-rs crates.
//!
//! Zero internal dependencies (only depends on vercel-ai-provider for LLM message types).

// === Version isolation layer: re-export vercel-ai types as version-agnostic aliases ===
// All crates reference via these aliases — never use vercel_ai_provider::* directly.
// Upgrading vercel-ai v5 only requires changing these re-exports.
pub use vercel_ai_provider::AssistantContentPart as AssistantContent;
pub use vercel_ai_provider::FilePart as FileContent;
pub use vercel_ai_provider::LanguageModelV4Message as LlmMessage;
pub use vercel_ai_provider::LanguageModelV4Prompt as LlmPrompt;
pub use vercel_ai_provider::ReasoningPart as ReasoningContent;
pub use vercel_ai_provider::TextPart as TextContent;
pub use vercel_ai_provider::ToolCallPart as ToolCallContent;
pub use vercel_ai_provider::ToolContentPart as ToolContent;
pub use vercel_ai_provider::ToolResultPart as ToolResultContent;
pub use vercel_ai_provider::UserContentPart as UserContent;

// === Modules ===
mod agent;
mod command;
mod extended;
mod hook;
mod id;
mod log;
mod message;
mod permission;
mod plugin;
mod provider;
mod sandbox;
pub mod side_query;
mod stream;
mod task;
mod thinking;
mod token;
mod tool;

// === Re-exports ===

// Agent types
pub use agent::AgentDefinition;
pub use agent::AgentIsolation;
pub use agent::AgentTypeId;
pub use agent::MemoryScope;
pub use agent::ModelInheritance;
pub use agent::ModelSource;
pub use agent::SubagentType;

// Command types
pub use command::CommandAvailability;
pub use command::CommandBase;
pub use command::CommandContext;
pub use command::CommandSafety;
pub use command::CommandSource;
pub use command::CommandType;
pub use command::LocalCommandData;
pub use command::PromptCommandData;

// Hook types
pub use hook::HookEventType;
pub use hook::HookOutcome;
pub use hook::HookResult;
pub use hook::HookScope;

// ID types
pub use id::AgentId;
pub use id::SessionId;
pub use id::TaskId;

// Log types
pub use log::Entrypoint;
pub use log::LogOption;
pub use log::SerializedMessage;
pub use log::UserType;

// Message types
pub use message::ApiError;
pub use message::AssistantMessage;
pub use message::AttachmentMessage;
pub use message::CompactTrigger;
pub use message::Message;
pub use message::MessageKind;
pub use message::MessageOrigin;
pub use message::PartialCompactDirection;
pub use message::PreservedSegment;
pub use message::ProgressMessage;
pub use message::StopReason;
pub use message::SystemAgentsKilledMessage;
pub use message::SystemApiErrorMessage;
pub use message::SystemApiMetricsMessage;
pub use message::SystemAwaySummaryMessage;
pub use message::SystemBridgeStatusMessage;
pub use message::SystemCompactBoundaryMessage;
pub use message::SystemInformationalMessage;
pub use message::SystemLocalCommandMessage;
pub use message::SystemMemorySavedMessage;
pub use message::SystemMessage;
pub use message::SystemMessageLevel;
pub use message::SystemMicrocompactBoundaryMessage;
pub use message::SystemPermissionRetryMessage;
pub use message::SystemScheduledTaskFireMessage;
pub use message::SystemStopHookSummaryMessage;
pub use message::SystemTurnDurationMessage;
pub use message::TombstoneMessage;
pub use message::ToolResultMessage;
pub use message::ToolUseSummaryMessage;
pub use message::UserMessage;

// Permission types
pub use permission::AdditionalWorkingDir;
pub use permission::ClassifierBehavior;
pub use permission::ClassifierUsage;
pub use permission::PendingClassifierCheck;
pub use permission::PermissionBehavior;
pub use permission::PermissionDecision;
pub use permission::PermissionDecisionReason;
pub use permission::PermissionRule;
pub use permission::PermissionRuleSource;
pub use permission::PermissionRuleValue;
pub use permission::PermissionRulesBySource;
pub use permission::PermissionUpdate;
pub use permission::PermissionUpdateDestination;
pub use permission::ToolPermissionContext;
pub use permission::WorkingDirectorySource;
pub use permission::content_matches;
pub use permission::matches_rule;
pub use permission::parse_rule_pattern;
pub use permission::tool_matches_pattern;

// Plugin types
pub use plugin::BuiltinPluginDefinition;

// Provider & model types
pub use provider::ApplyPatchToolType;
pub use provider::Capability;
pub use provider::CapabilitySet;
pub use provider::ModelRole;
pub use provider::ModelSpec;
pub use provider::ProviderApi;
pub use provider::WireApi;

// Sandbox types
pub use sandbox::SandboxMode;

// Side-query types (data only; async trait in coco-tool)
pub use side_query::SideQueryMessage;
pub use side_query::SideQueryRequest;
pub use side_query::SideQueryResponse;
pub use side_query::SideQueryRole;
pub use side_query::SideQueryStopReason;
pub use side_query::SideQueryToolDef;
pub use side_query::SideQueryToolUse;
pub use side_query::SideQueryUsage;

// Stream types
pub use stream::RequestStartEvent;
pub use stream::StreamEvent;
pub use stream::StreamingThinking;
pub use stream::StreamingToolUse;
pub use stream::TaskBudget;

// Task types
pub use task::TaskStateBase;
pub use task::TaskStatus;
pub use task::TaskType;
pub use task::generate_task_id;

// Thinking types
pub use thinking::ReasoningEffort;
pub use thinking::ThinkingLevel;

// Token types
pub use token::ModelUsage;
pub use token::TokenUsage;

// Tool types
pub use tool::AGENT_WORKTREE_BRANCH_PREFIX;
pub use tool::MCP_TOOL_PREFIX;
pub use tool::MCP_TOOL_SEPARATOR;
pub use tool::ToolId;
pub use tool::ToolInputSchema;
pub use tool::ToolName;
pub use tool::ToolProgress;
pub use tool::ToolResult;

// Extended types (ported from TS hooks.ts, command.ts, permissions.ts, logs.ts)
pub use extended::{
    // Log / transcript extended
    AgentColorEntry,
    AgentNameEntry,
    AgentSettingEntry,
    // Hook extended
    AiTitleEntry,
    AttributionSnapshotEntry,
    CommandBaseExt,
    CommandKind,
    CommandResultDisplay,
    CustomTitleEntry,
    FileAttributionState,
    HookBlockingError,
    HookProgress,
    // Permission extended
    PermissionCommandMetadata,
    PermissionDecisionReasonExt,
    PermissionExplanation,
    PermissionRequestResult,
    PermissionResult,
    PersistedWorktreeSession,
    PrLinkEntry,
    PromptCommandDataExt,
    PromptOption,
    PromptRequest,
    PromptResponse,
    ResumeEntrypoint,
    RiskLevel,
    SandboxOverrideReason,
    SessionMode,
    SummaryEntry,
    TagEntry,
    TaskSummaryEntry,
    ToolPermissionContextExt,
    TranscriptEntry,
    TranscriptMessage,
};

/// Permission mode (top-level because it's used by both message and permission modules).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Default,
    Plan,
    BypassPermissions,
    DontAsk,
    AcceptEdits,
    /// Feature-gated auto mode.
    Auto,
    /// Internal: escalate to parent agent.
    Bubble,
}
