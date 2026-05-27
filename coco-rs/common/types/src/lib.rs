//! Foundation types shared across all coco-rs crates.
//!
//! **Source-level vercel-ai-free.** Provider DTOs (LlmMessage, content
//! parts, ProviderOptions, StopReason, FinishReason, …) come in through
//! `coco-llm-types`, the dedicated DTO seam. This crate names them but
//! never imports `vercel_ai_provider::*` directly. Upgrading the SDK
//! requires editing only `common/llm-types/src/lib.rs` plus the runtime
//! seam in `services/inference`; this crate stays unchanged. See
//! `scripts/check-vercel-ai-seam.sh`.

// === Modules ===
mod agent;
mod agent_ipc;
mod app_state;
mod attachment_kind;
mod cache;
mod client_request;
mod command;
mod event;
mod extended;
pub mod features;
mod fork_label;
mod hook;
mod id;
mod jsonrpc;
mod log;
pub mod messages;
// Flat re-export at the crate root: `coco_types::Message` reads better
// than `coco_types::messages::Message`, and mirrors how every other
// coco-types module is surfaced. The submodule path
// (`coco_types::messages::*`) stays available for the operations-layer
// re-export in `coco-messages`.
pub use messages::*;
mod permission;
mod plugin;
mod provider;
mod rate_limit;
mod sandbox;
mod sdk_hook_output;
mod server_request;
pub mod side_query;
mod stream;
mod task;
mod task_list;
mod thinking;
mod token;
mod tool;
mod tool_filter;
mod wire_tagged;

// === Re-exports ===

// App-state (cross-turn shared state carried on ToolUseContext)
pub use app_state::AppStatePatch;
pub use app_state::AppStateReadHandle;
pub use app_state::ElicitationGuard;
pub use app_state::PendingPermissionGuard;
pub use app_state::PromptSuggestion;
pub use app_state::ToolAppState;

// Per-provider rate-limit state (lives on `ToolAppState.rate_limits`).
pub use rate_limit::RateLimitEntry;

// Attachment taxonomy (full TS `Attachment.type` catalog + coverage)
pub use attachment_kind::AttachmentEvent;
pub use attachment_kind::AttachmentKind;
pub use attachment_kind::Coverage;
pub use attachment_kind::SdkConsumption;
pub use attachment_kind::coverage_of;
pub use attachment_kind::sdk_consumption_of;

// Prompt-cache shared types (consumed by services/inference + app/query;
// adapter mirrors live in vercel-ai-anthropic — see prompt-cache-design.md §7)
pub use cache::AccountKind;
pub use cache::BetaCapability;
pub use cache::CacheScope;
pub use cache::CacheTtl;
pub use cache::PromptCacheConfig;
pub use cache::PromptCacheMode;

// Agent types
pub use agent::AgentColorName;
pub use agent::AgentDefinition;
pub use agent::AgentIsolation;
pub use agent::AgentMcpServerSpec;
pub use agent::AgentSource;
pub use agent::AgentTypeId;
pub use agent::MemoryScope;
pub use agent::ModelInheritance;
pub use agent::ModelSource;
pub use agent::SubagentType;
pub use agent::ToolAllowList;

// Inter-agent IPC (mailbox protocol + sub-agent state snapshots)
pub use agent_ipc::IdleReason;
pub use agent_ipc::StandaloneAgentContext;
pub use agent_ipc::SubAgentState;
pub use agent_ipc::SubAgentStatus;
pub use agent_ipc::SubagentRuntimeSnapshot;
pub use agent_ipc::TaskEntry;
pub use agent_ipc::TeamContext;
pub use agent_ipc::TeammateEntry;
pub use agent_ipc::TeammateProtocolContent;
pub use agent_ipc::TeammateProtocolMessage;

// Event types (three-layer CoreEvent system; see event-system-design.md)
pub use event::AgentInfo;
pub use event::AgentStreamEvent;
pub use event::AgentsDialogEntry;
pub use event::AgentsDialogPayload;
pub use event::AgentsKilledParams;
pub use event::CancelReason;
pub use event::CompactionFailedParams;
pub use event::CompactionHookType;
pub use event::CompactionPhase;
pub use event::CompactionPhaseParams;
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
pub use event::MemoryDialogEntry;
pub use event::MemoryDialogRowKind;
pub use event::MemoryDialogScope;
pub use event::ModelFallbackParams;
pub use event::ModelRoleChangedParams;
pub use event::NotificationMethod;
pub use event::PermissionDenialInfo;
pub use event::PermissionDisplayInput;
pub use event::PermissionModeChangedParams;
pub use event::PersistedFileError;
pub use event::PersistedFileInfo;
pub use event::PlanApprovalRequestedParams;
pub use event::PlanModeChangedParams;
pub use event::PluginInit;
pub use event::RateLimitParams;
pub use event::RateLimitStatus;
pub use event::ReasoningMetadataAttachedParams;
pub use event::RewindCompletedParams;
pub use event::RewindDiffStatsPayload;
pub use event::RewindRowMetadata;
pub use event::SandboxStateChangedParams;
pub use event::ServerNotification;
pub use event::SessionEndedParams;
pub use event::SessionModelUsage;
pub use event::SessionResultParams;
pub use event::SessionStartedParams;
pub use event::SessionState;
pub use event::SkillLock;
pub use event::SkillLockSource;
pub use event::SkillOverrideState;
pub use event::SkillOverridesSaveErrorKind;
pub use event::SkillOverridesSaveResult;
pub use event::SkillsDialogEntry;
pub use event::SkillsDialogPayload;
pub use event::SkillsDialogSource;
pub use event::SlashCommandStatusKind;
pub use event::SummarizeCompletedParams;
pub use event::TaskCompletedParams;
pub use event::TaskCompletionStatus;
pub use event::TaskPanelChangedParams;
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
pub use client_request::AgentInterruptCurrentWorkParams;
pub use client_request::ApprovalDecision;
pub use client_request::ApprovalResolveParams;
pub use client_request::CancelRequestParams;
pub use client_request::ClientRequest;
pub use client_request::ClientRequestMethod;
pub use client_request::ConfigApplyFlagsParams;
pub use client_request::ConfigWriteParams;
pub use client_request::ElicitationResolveParams;
pub use client_request::HookCallbackMatcher;
pub use client_request::InitializeParams;
pub use client_request::McpReconnectParams;
pub use client_request::McpSetServersParams;
pub use client_request::McpToggleParams;
pub use client_request::RewindFilesParams;
pub use client_request::SdkAgentDefinition;
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

// SDK hook callback output (TS-canonical wire shape; mirrors
// `hookJSONOutputSchema`). Single source of truth for the SDK
// boundary and for hook orchestration's stdout parser.
pub use sdk_hook_output::ElicitationAction;
pub use sdk_hook_output::HookCallbackResult;
pub use sdk_hook_output::HookDecision;
pub use sdk_hook_output::HookSpecificOutput;
pub use sdk_hook_output::McpRouteMessageResult;
pub use sdk_hook_output::PermissionRequestDecision;
pub use sdk_hook_output::SdkHookOutput;

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
pub use server_request::RequestElicitationParams as ServerRequestElicitationParams;
pub use server_request::RequestUserInputParams as ServerRequestUserInputParams;
pub use server_request::RewindFilesResult;
pub use server_request::SdkAccountInfo;
pub use server_request::SdkAgentInfo;
pub use server_request::SdkModelInfo;
pub use server_request::SdkSessionSummary;
pub use server_request::SdkSlashCommand;
pub use server_request::ServerCancelRequestParams;
pub use server_request::ServerRequest;
pub use server_request::ServerRequestMethod;
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
pub use command::CommandTypeTag;
pub use command::LocalCommandData;
pub use command::PromptCommandData;
pub use command::SlashCommandInfo;

// Hook types
pub use hook::HookEventType;
pub use hook::HookOutcome;
pub use hook::HookScope;

// ID types
pub use id::AgentId;
pub use id::SessionId;
pub use id::TaskId;

// Log types
pub use log::Entrypoint;
pub use log::LogOption;
pub use log::UserType;

/// How compaction was triggered.
///
/// Stays in `coco-types` (rather than `coco-messages`) because
/// `event::CompactionPhaseParams` references it; the rest of the message
/// family lives in `coco-messages`.
///
/// Mirrors the TS taxonomy: manual `/compact`, threshold-based auto, PTL-413
/// reactive recovery, gap-based time-based microcompact, session-memory
/// short-circuit (no LLM), and staged context-collapse commit.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTrigger {
    Manual,
    Auto,
    Reactive,
    TimeBased,
    SessionMemory,
    ContextCollapse,
}

// Permission types
pub use permission::AdditionalWorkingDir;
pub use permission::ClassifierBehavior;
pub use permission::ClassifierUsage;
pub use permission::PendingClassifierCheck;
pub use permission::PermissionAskChoice;
pub use permission::PermissionBehavior;
pub use permission::PermissionDecision;
pub use permission::PermissionDecisionReason;
pub use permission::PermissionRule;
pub use permission::PermissionRuleSource;
pub use permission::PermissionRuleValue;
pub use permission::PermissionRulesBySource;
pub use permission::PermissionUpdate;
pub use permission::PermissionUpdateDestination;
pub use permission::ToolCheckResult;
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
pub use provider::LlmModelSelection;
pub use provider::ModelRole;
pub use provider::ModelSpec;
pub use provider::ProviderApi;
pub use provider::ProviderModelSelection;
pub use provider::WireApi;

// Sandbox types
pub use sandbox::SandboxMode;

// Feature gates
pub use features::Feature;
pub use features::FeatureSpec;
pub use features::Features;
pub use features::Stage as FeatureStage;
pub use features::all_features;
pub use features::feature_for_key;
pub use features::is_known_feature_key;

// Fork-label discriminator (used by logs / telemetry / transcripts to
// identify framework-spawned, cache-shared side-channel queries).
pub use fork_label::ForkLabel;

// Tool filter pipeline (Layers 2 + 4)
pub use tool_filter::ToolFilter;
pub use tool_filter::ToolOverrides;

// Side-query types (data only; async trait in coco-tool-runtime)
pub use side_query::CacheSafeParams;
pub use side_query::SideQueryMessage;
pub use side_query::SideQueryOutputFormat;
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
pub use task::BackendType;
pub use task::BgAgentExtras;
pub use task::DreamExtras;
pub use task::FieldUpdate;
pub use task::MessageRole;
pub use task::RemoteTeammateExtras;
pub use task::ShellExtras;
pub use task::TaskActivity;
pub use task::TaskExtras;
pub use task::TaskIdentity;
pub use task::TaskProgress;
pub use task::TaskStateBase;
pub use task::TaskStatus;
pub use task::TaskType;
pub use task::TeammateExtras;
pub use task::TeammateRef;
pub use task::TeammateTaskMessage;
pub use task::generate_bg_agent_id;
pub use task::generate_task_id;
pub use task::task_type_wire;
pub use task_list::ExpandedView;
pub use task_list::TaskClaimOutcome;
pub use task_list::TaskListStatus;
pub use task_list::TaskRecord;
pub use task_list::TaskRecordUpdate;
pub use task_list::TodoRecord;

// Thinking types
pub use thinking::ReasoningEffort;
pub use thinking::ThinkingLevel;

// Token types
pub use token::InputTokens;
pub use token::ModelUsage;
pub use token::OutputTokens;
pub use token::SessionModelUsageEntry;
pub use token::SessionUsageSnapshot;
pub use token::SessionUsageTotals;
pub use token::TokenUsage;

// Tool types (ToolResult moved to coco-messages because new_messages: Vec<Message>)
pub use tool::AGENT_WORKTREE_BRANCH_PREFIX;
pub use tool::MCP_TOOL_PREFIX;
pub use tool::MCP_TOOL_SEPARATOR;
pub use tool::ToolId;
pub use tool::ToolInputSchema;
pub use tool::ToolName;
pub use tool::ToolProgress;
pub use tool::legacy_tool_name_aliases_of;
pub use tool::normalize_legacy_tool_name;

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

impl PermissionMode {
    /// Next mode when the user presses Shift+Tab.
    ///
    /// TS: `getNextPermissionMode()` in utils/permissions/getNextPermissionMode.ts
    ///
    /// Cycle: `Default → AcceptEdits → Plan → [BypassPermissions] → [Auto] → Default`.
    /// Optional modes are skipped when their gate flag is false.
    pub fn next_in_cycle(self, bypass_available: bool, auto_available: bool) -> Self {
        match self {
            Self::Default => Self::AcceptEdits,
            Self::AcceptEdits => Self::Plan,
            Self::Plan => {
                if bypass_available {
                    Self::BypassPermissions
                } else if auto_available {
                    Self::Auto
                } else {
                    Self::Default
                }
            }
            Self::BypassPermissions => {
                if auto_available {
                    Self::Auto
                } else {
                    Self::Default
                }
            }
            // Auto, DontAsk, Bubble, and any future mode fall back to Default.
            Self::Auto | Self::DontAsk | Self::Bubble => Self::Default,
        }
    }
}
