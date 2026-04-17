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
mod client_request;
mod command;
mod event;
mod extended;
mod hook;
mod id;
mod jsonrpc;
mod log;
mod message;
mod permission;
mod plugin;
mod provider;
mod sandbox;
mod server_request;
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

// Event types (three-layer CoreEvent system; see event-system-design.md)
pub use event::AgentInfo;
pub use event::AgentStreamEvent;
pub use event::AgentsKilledParams;
pub use event::CompactionFailedParams;
pub use event::ContentDeltaParams;
pub use event::ContextClearedParams;
pub use event::ContextCompactedParams;
pub use event::ContextUsageWarningParams;
pub use event::CoreEvent;
pub use event::CostWarningParams;
pub use event::ElicitationCompleteParams;
pub use event::ErrorParams;
pub use event::FastModeState;
pub use event::FileChangeInfo;
pub use event::FileChangeKind;
pub use event::FilesPersistedParams;
pub use event::HookOutcomeStatus;
pub use event::HookProgressParams;
pub use event::HookResponseParams;
pub use event::HookStartedParams;
pub use event::IdeDiagnosticsUpdatedParams;
pub use event::IdeSelectionChangedParams;
pub use event::ItemStatus;
pub use event::LocalCommandOutputParams;
pub use event::McpServerInit;
pub use event::McpStartupCompleteParams;
pub use event::McpStartupStatusParams;
pub use event::ModelFallbackParams;
pub use event::PermissionDenialInfo;
pub use event::PermissionModeChangedParams;
pub use event::PersistedFileError;
pub use event::PersistedFileInfo;
pub use event::PlanModeChangedParams;
pub use event::PluginInit;
pub use event::RateLimitParams;
pub use event::RateLimitStatus;
pub use event::RewindCompletedParams;
pub use event::SandboxStateChangedParams;
pub use event::ServerNotification;
pub use event::SessionEndedParams;
pub use event::SessionModelUsage;
pub use event::SessionResultParams;
pub use event::SessionStartedParams;
pub use event::SessionState;
pub use event::SubagentBackgroundedParams;
pub use event::SubagentCompletedParams;
pub use event::SubagentProgressParams;
pub use event::SubagentSpawnedParams;
pub use event::SummarizeCompletedParams;
pub use event::TaskCompletedParams;
pub use event::TaskCompletionStatus;
pub use event::TaskProgressParams;
pub use event::TaskStartedParams;
pub use event::TaskUsage;
pub use event::ThreadItem;
pub use event::ThreadItemDetails;
pub use event::ToolProgressParams;
pub use event::ToolUseSummaryParams;
pub use event::TuiOnlyEvent;
pub use event::TurnCompletedParams;
pub use event::TurnFailedParams;
pub use event::TurnInterruptedParams;
pub use event::TurnStartedParams;
pub use event::WorktreeEnteredParams;
pub use event::WorktreeExitedParams;

// Client request types (Phase 2 — SDK control protocol, SDK → agent)
pub use client_request::ApprovalDecision;
pub use client_request::ApprovalResolveParams;
pub use client_request::CancelRequestParams;
pub use client_request::ClientRequest;
pub use client_request::ConfigApplyFlagsParams;
pub use client_request::ConfigWriteParams;
pub use client_request::ElicitationResolveParams;
pub use client_request::HookCallbackMatcher;
pub use client_request::HookCallbackResponseParams as ClientHookCallbackResponseParams;
pub use client_request::InitializeParams;
pub use client_request::McpReconnectParams;
pub use client_request::McpRouteMessageResponseParams;
pub use client_request::McpSetServersParams;
pub use client_request::McpToggleParams;
pub use client_request::RewindFilesParams;
pub use client_request::SessionArchiveParams;
pub use client_request::SessionReadParams;
pub use client_request::SessionResumeParams;
pub use client_request::SessionStartParams;
pub use client_request::SetModelParams;
pub use client_request::SetPermissionModeParams;
pub use client_request::SetThinkingParams;
pub use client_request::StopTaskParams;
pub use client_request::TurnStartParams;
pub use client_request::UpdateEnvParams;
pub use client_request::UserInputResolveParams;

// Server request types (Phase 2 — SDK control protocol, agent → SDK)
pub use server_request::ApiProvider as SdkApiProvider;
pub use server_request::AskForApprovalParams as ServerAskForApprovalParams;
pub use server_request::ConfigReadResult;
pub use server_request::ContextUsageCategory;
pub use server_request::ContextUsageResult;
pub use server_request::EffortLevel as SdkEffortLevel;
pub use server_request::HookCallbackParams as ServerHookCallbackParams;
pub use server_request::InitializeResult;
pub use server_request::McpConnectionStatus;
pub use server_request::McpRouteMessageParams as ServerMcpRouteMessageParams;
pub use server_request::McpServerStatus;
pub use server_request::McpSetServersResult;
pub use server_request::McpStatusResult;
pub use server_request::MessageBreakdown;
pub use server_request::PluginReloadResult;
pub use server_request::RequestUserInputParams as ServerRequestUserInputParams;
pub use server_request::RewindFilesResult;
pub use server_request::SdkAccountInfo;
pub use server_request::SdkAgentInfo;
pub use server_request::SdkModelInfo;
pub use server_request::SdkSessionSummary;
pub use server_request::SdkSlashCommand;
pub use server_request::ServerCancelRequestParams;
pub use server_request::ServerRequest;
pub use server_request::SessionListResult;
pub use server_request::SessionReadResult;
pub use server_request::SessionResumeResult;
pub use server_request::SessionStartResult;
pub use server_request::TurnStartResult;

// JSON-RPC envelope types (Phase 2 — wire format)
pub use jsonrpc::JsonRpcError;
pub use jsonrpc::JsonRpcMessage;
pub use jsonrpc::JsonRpcNotification;
pub use jsonrpc::JsonRpcRequest;
pub use jsonrpc::JsonRpcResponse;
pub use jsonrpc::RequestId;
pub use jsonrpc::error_codes;

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
///
/// Wire format is camelCase to match TS `PermissionModeSchema` at
/// `coreSchemas.ts:337-347`: `z.enum(['default', 'acceptEdits',
/// 'bypassPermissions', 'plan', 'dontAsk'])`. The serde aliases on the
/// drifting variants accept legacy snake_case input so old session
/// transcripts deserialize cleanly.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    Plan,
    #[serde(alias = "bypass_permissions")]
    BypassPermissions,
    #[serde(alias = "dont_ask")]
    DontAsk,
    #[serde(alias = "accept_edits")]
    AcceptEdits,
    /// Feature-gated auto mode.
    Auto,
    /// Internal: escalate to parent agent.
    Bubble,
}
