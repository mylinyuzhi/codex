"""Generated protocol types for the coco SDK.

These types mirror the Rust `coco-app-server-protocol` crate.
Regenerate with: `scripts/generate_python.sh`

Source schemas: coco-rs/app-server-protocol/schema/json/

DO NOT EDIT MANUALLY — changes will be overwritten by the generator.
"""

from __future__ import annotations

from enum import Enum
from typing import Any

from pydantic import BaseModel, Field

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------

class McpConnectionStatus(str, Enum):
    connected = 'connected'
    disconnected = 'disconnected'

class PermissionMode(str, Enum):
    default = 'default'
    auto = 'auto'
    bubble = 'bubble'

class SessionState(str, Enum):
    idle = 'idle'
    running = 'running'
    requires_action = 'requires_action'


# ---------------------------------------------------------------------------
# Item types
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# ThreadItem (tagged union with extra fields)
# ---------------------------------------------------------------------------

class ThreadItem(BaseModel):
    """A discrete operation within a turn."""

    id: str
    type: str
    model_config = {"extra": "allow"}

    def as_agent_message(self) -> AgentMessageItem | None:
        if self.type == 'agent_message':
            return AgentMessageItem.model_validate(self.model_extra or {})
        return None

    def as_reasoning(self) -> ReasoningItem | None:
        if self.type == 'reasoning':
            return ReasoningItem.model_validate(self.model_extra or {})
        return None

    def as_command_execution(self) -> CommandExecutionItem | None:
        if self.type == 'command_execution':
            return CommandExecutionItem.model_validate(self.model_extra or {})
        return None

    def as_file_change(self) -> FileChangeItem | None:
        if self.type == 'file_change':
            return FileChangeItem.model_validate(self.model_extra or {})
        return None

    def as_mcp_tool_call(self) -> McpToolCallItem | None:
        if self.type == 'mcp_tool_call':
            return McpToolCallItem.model_validate(self.model_extra or {})
        return None

    def as_web_search(self) -> WebSearchItem | None:
        if self.type == 'web_search':
            return WebSearchItem.model_validate(self.model_extra or {})
        return None

    def as_subagent(self) -> SubagentItem | None:
        if self.type == 'subagent':
            return SubagentItem.model_validate(self.model_extra or {})
        return None

    def as_tool_call(self) -> GenericToolCallItem | None:
        if self.type == 'tool_call':
            return GenericToolCallItem.model_validate(self.model_extra or {})
        return None

    def as_error_item(self) -> ErrorItem | None:
        if self.type == 'error':
            return ErrorItem.model_validate(self.model_extra or {})
        return None


# ---------------------------------------------------------------------------
# Server notification params
# ---------------------------------------------------------------------------

class SessionStartedParams(BaseModel):
    cwd: str
    model: str
    permission_mode: str
    protocol_version: str
    session_id: str
    version: str
    agents: list[str] = []
    api_key_source: str | None = None
    betas: list[str] | None = None
    fast_mode_state: FastModeState | None = None
    mcp_servers: list[McpServerInit] = []
    output_style: str | None = None
    plugins: list[PluginInit] = []
    skills: list[str] = []
    slash_commands: list[str] = []
    tools: list[str] = []

class SessionResultParams(BaseModel):
    duration_api_ms: int
    duration_ms: int
    session_id: str
    stop_reason: str
    total_cost_usd: float
    total_turns: int
    usage: TokenUsage
    errors: list[str] | None = None
    fast_mode_state: FastModeState | None = None
    is_error: bool = False
    model_usage: dict[str, SessionModelUsage] = {}
    num_api_calls: int | None = None
    permission_denials: list[PermissionDenialInfo] = []
    result: str | None = None
    structured_output: Any = None

class SessionEndedParams(BaseModel):
    reason: str

class TurnStartedParams(BaseModel):
    turn_number: int
    turn_id: str | None = None

class TurnCompletedParams(BaseModel):
    usage: TokenUsage
    turn_id: str | None = None

class TurnFailedParams(BaseModel):
    error: str

class TurnInterruptedNotifParams(BaseModel):
    turn_id: str | None = None

class ContentDeltaParams(BaseModel):
    delta: str
    item_id: str | None = None
    turn_id: str | None = None

class SubagentSpawnedParams(BaseModel):
    agent_id: str
    agent_type: str
    description: str
    color: str | None = None

class SubagentCompletedParams(BaseModel):
    agent_id: str
    result: str
    is_error: bool = False

class SubagentBackgroundedParams(BaseModel):
    agent_id: str
    output_file: str

class SubagentProgressParams(BaseModel):
    agent_id: str
    current_step: int | None = None
    message: str | None = None
    summary: str | None = None
    total_steps: int | None = None

class McpStartupStatusParams(BaseModel):
    server: str
    status: McpConnectionStatus

class McpStartupCompleteParams(BaseModel):
    servers: list[str]
    failed: list[str] = []

class ContextCompactedParams(BaseModel):
    removed_messages: int
    summary_tokens: int

class ContextUsageWarningParams(BaseModel):
    estimated_tokens: int
    percent_left: float
    warning_threshold: int

class CompactionFailedParams(BaseModel):
    error: str
    attempts: int = 0

class ContextClearedParams(BaseModel):
    new_mode: str | None = None

class TaskStartedParams(BaseModel):
    description: str
    task_id: str
    prompt: str | None = None
    task_type: str | None = None
    tool_use_id: str | None = None
    workflow_name: str | None = None

class TaskCompletedParams(BaseModel):
    output_file: str
    status: TaskCompletionStatus
    summary: str
    task_id: str
    tool_use_id: str | None = None
    usage: TaskUsage | None = None

class TaskProgressParams(BaseModel):
    description: str
    task_id: str
    usage: TaskUsage
    last_tool_name: str | None = None
    summary: str | None = None
    tool_use_id: str | None = None
    workflow_progress: list[Any] | None = None

class AgentsKilledParams(BaseModel):
    count: int
    agent_ids: list[str] = []

class ModelFallbackParams(BaseModel):
    from_model: str
    reason: str
    to_model: str

class PermissionModeChangedParams(BaseModel):
    mode: str
    bypass_available: bool = False

class ErrorParams(BaseModel):
    message: str
    category: str | None = None
    retryable: bool = False

class RateLimitParams(BaseModel):
    limit: int | None = None
    provider: str | None = None
    rate_limit_type: str | None = None
    remaining: int | None = None
    reset_at: int | None = None
    status: RateLimitStatus | None = None
    utilization: float | None = None

class IdeSelectionChangedParams(BaseModel):
    end_line: int
    file_path: str
    selected_text: str
    start_line: int

class IdeDiagnosticsUpdatedParams(BaseModel):
    file_path: str
    new_count: int
    diagnostics: list[Any] = []

class PlanModeChangedParams(BaseModel):
    entered: bool
    approved: bool | None = None
    plan_file: str | None = None

class RewindCompletedParams(BaseModel):
    messages_removed: int
    restored_files: int
    rewound_turn: int

class CostWarningParams(BaseModel):
    current_cost_cents: int
    threshold_cents: int
    budget_cents: int | None = None

class SandboxStateChangedParams(BaseModel):
    active: bool
    enforcement: str

class HookExecutedParams(BaseModel):
    hook_name: str
    hook_type: str

class HookStartedParams(BaseModel):
    hook_event: str
    hook_id: str
    hook_name: str

class HookProgressParams(BaseModel):
    hook_event: str
    hook_id: str
    hook_name: str
    output: str = ''
    stderr: str = ''
    stdout: str = ''

class HookResponseParams(BaseModel):
    hook_event: str
    hook_id: str
    hook_name: str
    outcome: HookOutcomeStatus
    output: str
    exit_code: int | None = None
    stderr: str = ''
    stdout: str = ''

class WorktreeEnteredParams(BaseModel):
    branch: str
    worktree_path: str

class WorktreeExitedParams(BaseModel):
    action: str
    worktree_path: str

class SummarizeCompletedParams(BaseModel):
    from_turn: int
    summary_tokens: int

class LocalCommandOutputParams(BaseModel):
    content: Any

class FilesPersistedParams(BaseModel):
    files: list[PersistedFileInfo]
    processed_at: str
    failed: list[PersistedFileError] = []

class ElicitationCompleteParams(BaseModel):
    elicitation_id: str
    mcp_server_name: str

class ToolUseSummaryParams(BaseModel):
    preceding_tool_use_ids: list[str]
    summary: str

class ToolProgressParams(BaseModel):
    elapsed_time_seconds: float
    tool_name: str
    tool_use_id: str
    parent_tool_use_id: str | None = None
    task_id: str | None = None


# ---------------------------------------------------------------------------
# Server notifications (tagged union)
# ---------------------------------------------------------------------------

class ServerNotification(BaseModel):
    """An event from the server. Use `method` to determine the event type."""

    method: str
    params: dict[str, Any] = {}

    def as_session_started(self) -> SessionStartedParams | None:
        if self.method == 'session/started':
            return SessionStartedParams.model_validate(self.params)
        return None

    def as_session_result(self) -> SessionResultParams | None:
        if self.method == 'session/result':
            return SessionResultParams.model_validate(self.params)
        return None

    def as_session_ended(self) -> SessionEndedParams | None:
        if self.method == 'session/ended':
            return SessionEndedParams.model_validate(self.params)
        return None

    def as_turn_started(self) -> TurnStartedParams | None:
        if self.method == 'turn/started':
            return TurnStartedParams.model_validate(self.params)
        return None

    def as_turn_completed(self) -> TurnCompletedParams | None:
        if self.method == 'turn/completed':
            return TurnCompletedParams.model_validate(self.params)
        return None

    def as_turn_failed(self) -> TurnFailedParams | None:
        if self.method == 'turn/failed':
            return TurnFailedParams.model_validate(self.params)
        return None

    def as_turn_interrupted(self) -> TurnInterruptedNotifParams | None:
        if self.method == 'turn/interrupted':
            return TurnInterruptedNotifParams.model_validate(self.params)
        return None

    def as_max_turns_reached(self) -> MaxTurnsReachedParams | None:
        if self.method == 'turn/maxReached':
            return MaxTurnsReachedParams.model_validate(self.params)
        return None

    def as_turn_retry(self) -> TurnRetryParams | None:
        if self.method == 'turn/retry':
            return TurnRetryParams.model_validate(self.params)
        return None

    def as_item_started(self) -> ItemEventParams | None:
        if self.method == 'item/started':
            return ItemEventParams.model_validate(self.params)
        return None

    def as_item_updated(self) -> ItemEventParams | None:
        if self.method == 'item/updated':
            return ItemEventParams.model_validate(self.params)
        return None

    def as_item_completed(self) -> ItemEventParams | None:
        if self.method == 'item/completed':
            return ItemEventParams.model_validate(self.params)
        return None

    def as_agent_message_delta(self) -> AgentMessageDeltaParams | None:
        if self.method == 'agentMessage/delta':
            return AgentMessageDeltaParams.model_validate(self.params)
        return None

    def as_reasoning_delta(self) -> ReasoningDeltaParams | None:
        if self.method == 'reasoning/delta':
            return ReasoningDeltaParams.model_validate(self.params)
        return None

    def as_subagent_spawned(self) -> SubagentSpawnedParams | None:
        if self.method == 'subagent/spawned':
            return SubagentSpawnedParams.model_validate(self.params)
        return None

    def as_subagent_completed(self) -> SubagentCompletedParams | None:
        if self.method == 'subagent/completed':
            return SubagentCompletedParams.model_validate(self.params)
        return None

    def as_subagent_backgrounded(self) -> SubagentBackgroundedParams | None:
        if self.method == 'subagent/backgrounded':
            return SubagentBackgroundedParams.model_validate(self.params)
        return None

    def as_subagent_progress(self) -> SubagentProgressParams | None:
        if self.method == 'subagent/progress':
            return SubagentProgressParams.model_validate(self.params)
        return None

    def as_mcp_startup_status(self) -> McpStartupStatusParams | None:
        if self.method == 'mcp/startupStatus':
            return McpStartupStatusParams.model_validate(self.params)
        return None

    def as_mcp_startup_complete(self) -> McpStartupCompleteParams | None:
        if self.method == 'mcp/startupComplete':
            return McpStartupCompleteParams.model_validate(self.params)
        return None

    def as_context_compacted(self) -> ContextCompactedParams | None:
        if self.method == 'context/compacted':
            return ContextCompactedParams.model_validate(self.params)
        return None

    def as_context_usage_warning(self) -> ContextUsageWarningParams | None:
        if self.method == 'context/usageWarning':
            return ContextUsageWarningParams.model_validate(self.params)
        return None

    def as_compaction_started(self) -> CompactionStartedParams | None:
        if self.method == 'context/compactionStarted':
            return CompactionStartedParams.model_validate(self.params)
        return None

    def as_compaction_failed(self) -> CompactionFailedParams | None:
        if self.method == 'context/compactionFailed':
            return CompactionFailedParams.model_validate(self.params)
        return None

    def as_context_cleared(self) -> ContextClearedParams | None:
        if self.method == 'context/cleared':
            return ContextClearedParams.model_validate(self.params)
        return None

    def as_task_started(self) -> TaskStartedParams | None:
        if self.method == 'task/started':
            return TaskStartedParams.model_validate(self.params)
        return None

    def as_task_completed(self) -> TaskCompletedParams | None:
        if self.method == 'task/completed':
            return TaskCompletedParams.model_validate(self.params)
        return None

    def as_task_progress(self) -> TaskProgressParams | None:
        if self.method == 'task/progress':
            return TaskProgressParams.model_validate(self.params)
        return None

    def as_agents_killed(self) -> AgentsKilledParams | None:
        if self.method == 'agents/killed':
            return AgentsKilledParams.model_validate(self.params)
        return None

    def as_model_fallback_started(self) -> ModelFallbackStartedParams | None:
        if self.method == 'model/fallbackStarted':
            return ModelFallbackStartedParams.model_validate(self.params)
        return None

    def as_model_fallback_completed(self) -> ModelFallbackCompletedParams | None:
        if self.method == 'model/fallbackCompleted':
            return ModelFallbackCompletedParams.model_validate(self.params)
        return None

    def as_fast_mode_changed(self) -> FastModeChangedParams | None:
        if self.method == 'model/fastModeChanged':
            return FastModeChangedParams.model_validate(self.params)
        return None

    def as_permission_mode_changed(self) -> PermissionModeChangedParams | None:
        if self.method == 'permission/modeChanged':
            return PermissionModeChangedParams.model_validate(self.params)
        return None

    def as_prompt_suggestion(self) -> PromptSuggestionParams | None:
        if self.method == 'prompt/suggestion':
            return PromptSuggestionParams.model_validate(self.params)
        return None

    def as_error(self) -> ErrorNotificationParams | None:
        if self.method == 'error':
            return ErrorNotificationParams.model_validate(self.params)
        return None

    def as_rate_limit(self) -> RateLimitParams | None:
        if self.method == 'rateLimit':
            return RateLimitParams.model_validate(self.params)
        return None

    def as_keep_alive(self) -> KeepAliveNotifParams | None:
        if self.method == 'keepAlive':
            return KeepAliveNotifParams.model_validate(self.params)
        return None

    def as_ide_selection_changed(self) -> IdeSelectionChangedParams | None:
        if self.method == 'ide/selectionChanged':
            return IdeSelectionChangedParams.model_validate(self.params)
        return None

    def as_ide_diagnostics_updated(self) -> IdeDiagnosticsUpdatedParams | None:
        if self.method == 'ide/diagnosticsUpdated':
            return IdeDiagnosticsUpdatedParams.model_validate(self.params)
        return None

    def as_plan_mode_changed(self) -> PlanModeChangedParams | None:
        if self.method == 'plan/modeChanged':
            return PlanModeChangedParams.model_validate(self.params)
        return None

    def as_queue_state_changed(self) -> QueueStateChangedParams | None:
        if self.method == 'queue/stateChanged':
            return QueueStateChangedParams.model_validate(self.params)
        return None

    def as_command_queued(self) -> CommandQueuedParams | None:
        if self.method == 'queue/commandQueued':
            return CommandQueuedParams.model_validate(self.params)
        return None

    def as_command_dequeued(self) -> CommandDequeuedParams | None:
        if self.method == 'queue/commandDequeued':
            return CommandDequeuedParams.model_validate(self.params)
        return None

    def as_rewind_completed(self) -> RewindCompletedParams | None:
        if self.method == 'rewind/completed':
            return RewindCompletedParams.model_validate(self.params)
        return None

    def as_rewind_failed(self) -> RewindFailedParams | None:
        if self.method == 'rewind/failed':
            return RewindFailedParams.model_validate(self.params)
        return None

    def as_cost_warning(self) -> CostWarningParams | None:
        if self.method == 'cost/warning':
            return CostWarningParams.model_validate(self.params)
        return None

    def as_sandbox_state_changed(self) -> SandboxStateChangedParams | None:
        if self.method == 'sandbox/stateChanged':
            return SandboxStateChangedParams.model_validate(self.params)
        return None

    def as_sandbox_violations_detected(self) -> SandboxViolationsDetectedParams | None:
        if self.method == 'sandbox/violationsDetected':
            return SandboxViolationsDetectedParams.model_validate(self.params)
        return None

    def as_agents_registered(self) -> AgentsRegisteredParams | None:
        if self.method == 'agents/registered':
            return AgentsRegisteredParams.model_validate(self.params)
        return None

    def as_hook_executed(self) -> HookExecutedParams | None:
        if self.method == 'hook/executed':
            return HookExecutedParams.model_validate(self.params)
        return None

    def as_worktree_entered(self) -> WorktreeEnteredParams | None:
        if self.method == 'worktree/entered':
            return WorktreeEnteredParams.model_validate(self.params)
        return None

    def as_worktree_exited(self) -> WorktreeExitedParams | None:
        if self.method == 'worktree/exited':
            return WorktreeExitedParams.model_validate(self.params)
        return None

    def as_summarize_completed(self) -> SummarizeCompletedParams | None:
        if self.method == 'summarize/completed':
            return SummarizeCompletedParams.model_validate(self.params)
        return None

    def as_summarize_failed(self) -> SummarizeFailedParams | None:
        if self.method == 'summarize/failed':
            return SummarizeFailedParams.model_validate(self.params)
        return None

    def as_stream_stall_detected(self) -> StreamStallDetectedParams | None:
        if self.method == 'stream/stallDetected':
            return StreamStallDetectedParams.model_validate(self.params)
        return None

    def as_stream_watchdog_warning(self) -> StreamWatchdogWarningParams | None:
        if self.method == 'stream/watchdogWarning':
            return StreamWatchdogWarningParams.model_validate(self.params)
        return None

    def as_stream_request_end(self) -> StreamRequestEndParams | None:
        if self.method == 'stream/requestEnd':
            return StreamRequestEndParams.model_validate(self.params)
        return None

    def as_hook_started(self) -> HookStartedParams | None:
        if self.method == 'hook/started':
            return HookStartedParams.model_validate(self.params)
        return None

    def as_hook_progress(self) -> HookProgressParams | None:
        if self.method == 'hook/progress':
            return HookProgressParams.model_validate(self.params)
        return None

    def as_hook_response(self) -> HookResponseParams | None:
        if self.method == 'hook/response':
            return HookResponseParams.model_validate(self.params)
        return None

    def as_session_state_changed(self) -> SessionStateChangedParams | None:
        if self.method == 'session/stateChanged':
            return SessionStateChangedParams.model_validate(self.params)
        return None

    def as_local_command_output(self) -> LocalCommandOutputParams | None:
        if self.method == 'localCommand/output':
            return LocalCommandOutputParams.model_validate(self.params)
        return None

    def as_files_persisted(self) -> FilesPersistedParams | None:
        if self.method == 'files/persisted':
            return FilesPersistedParams.model_validate(self.params)
        return None

    def as_elicitation_complete(self) -> ElicitationCompleteParams | None:
        if self.method == 'elicitation/complete':
            return ElicitationCompleteParams.model_validate(self.params)
        return None

    def as_tool_use_summary(self) -> ToolUseSummaryParams | None:
        if self.method == 'tool/useSummary':
            return ToolUseSummaryParams.model_validate(self.params)
        return None

    def as_tool_progress(self) -> ToolProgressParams | None:
        if self.method == 'tool/progress':
            return ToolProgressParams.model_validate(self.params)
        return None


# ---------------------------------------------------------------------------
# Server requests (server -> client, require response)
# ---------------------------------------------------------------------------

class AskForApprovalParams(BaseModel):
    input: Any
    request_id: str
    tool_name: str
    tool_use_id: str
    agent_id: str | None = None
    blocked_path: str | None = None
    decision_reason: str | None = None
    description: str | None = None
    display_name: str | None = None
    permission_suggestions: list[Any] | None = None
    title: str | None = None

class RequestUserInputParams(BaseModel):
    prompt: str
    request_id: str
    choices: list[str] | None = None
    default: str | None = None
    description: str | None = None

class McpRouteMessageParams(BaseModel):
    message: Any
    request_id: str
    server_name: str

class HookCallbackParams(BaseModel):
    callback_id: str
    input: Any
    request_id: str
    tool_use_id: str | None = None

class ServerCancelRequestParams(BaseModel):
    request_id: str
    reason: str | None = None


class ServerRequest(BaseModel):
    """A request from the server that requires a client response."""

    method: str
    params: dict[str, Any] = {}

    def as_ask_for_approval(self) -> AskForApprovalParams | None:
        if self.method == 'approval/askForApproval':
            return AskForApprovalParams.model_validate(self.params)
        return None

    def as_request_user_input(self) -> RequestUserInputParams | None:
        if self.method == 'input/requestUserInput':
            return RequestUserInputParams.model_validate(self.params)
        return None

    def as_hook_callback(self) -> HookCallbackParams | None:
        if self.method == 'hook/callback':
            return HookCallbackParams.model_validate(self.params)
        return None

    def as_mcp_route_message(self) -> McpRouteMessageParams | None:
        if self.method == 'mcp/routeMessage':
            return McpRouteMessageParams.model_validate(self.params)
        return None

    def as_cancel_request(self) -> ServerCancelRequestParams | None:
        if self.method == 'control/cancelRequest':
            return ServerCancelRequestParams.model_validate(self.params)
        return None


# ---------------------------------------------------------------------------
# MCP server config types
# ---------------------------------------------------------------------------

class StdioMcpServerConfig(BaseModel):
    """Subprocess-based MCP server (stdio transport)."""

    type: str = "stdio"
    command: str
    args: list[str] = []
    env: dict[str, str] | None = None


class SseMcpServerConfig(BaseModel):
    """SSE-based MCP server."""

    type: str = "sse"
    url: str


class HttpMcpServerConfig(BaseModel):
    """HTTP-based MCP server."""

    type: str = "http"
    url: str


McpServerConfig = StdioMcpServerConfig | SseMcpServerConfig | HttpMcpServerConfig

# ---------------------------------------------------------------------------
# Config types
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Hook input/output types
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Client request params
# ---------------------------------------------------------------------------

class InitializeParams(BaseModel):
    agent_progress_summaries: bool | None = None
    agents: dict[str, Any] | None = None
    append_system_prompt: str | None = None
    hooks: dict[str, list[HookCallbackMatcher]] | None = None
    json_schema: Any = None
    prompt_suggestions: bool | None = None
    sdk_mcp_servers: list[str] | None = None
    system_prompt: str | None = None

class SessionStartParams(BaseModel):
    append_system_prompt: str | None = None
    cwd: str | None = None
    initial_prompt: str | None = None
    max_budget_usd: float | None = None
    max_turns: int | None = None
    model: str | None = None
    permission_mode: PermissionMode | None = None
    system_prompt: str | None = None

class SessionResumeParams(BaseModel):
    session_id: str

class SessionReadParams(BaseModel):
    session_id: str
    cursor: str | None = None
    limit: int | None = None

class SessionArchiveParams(BaseModel):
    session_id: str

class TurnStartParams(BaseModel):
    prompt: str
    permission_mode: PermissionMode | None = None
    thinking_level: ThinkingLevel | None = None

class ApprovalResolveParams(BaseModel):
    decision: ApprovalDecision
    request_id: str
    feedback: str | None = None
    permission_update: PermissionUpdate | None = None
    updated_input: Any = None

class UserInputResolveParams(BaseModel):
    answer: str
    request_id: str

class ElicitationResolveParams(BaseModel):
    approved: bool
    mcp_server_name: str
    request_id: str
    values: dict[str, Any] = {}

class SetModelParams(BaseModel):
    model: str | None = None

class SetPermissionModeParams(BaseModel):
    mode: PermissionMode
    ultraplan: bool | None = None

class SetThinkingParams(BaseModel):
    thinking_level: ThinkingLevel | None = None

class StopTaskParams(BaseModel):
    task_id: str

class RewindFilesParams(BaseModel):
    user_message_id: str
    dry_run: bool = False

class UpdateEnvParams(BaseModel):
    env: dict[str, str]

class CancelRequestParams(BaseModel):
    request_id: str
    reason: str | None = None

class ConfigWriteParams(BaseModel):
    key: str
    value: Any
    scope: str | None = None

class HookCallbackResponseParams(BaseModel):
    callback_id: str
    output: Any

class McpRouteMessageResponseParams(BaseModel):
    message: Any
    request_id: str

class McpSetServersParams(BaseModel):
    servers: dict[str, Any]

class McpReconnectParams(BaseModel):
    server_name: str

class McpToggleParams(BaseModel):
    enabled: bool
    server_name: str

class ConfigApplyFlagsParams(BaseModel):
    settings: dict[str, Any]


# ---------------------------------------------------------------------------
# Client request wrappers
# ---------------------------------------------------------------------------

class InitializeRequest(BaseModel):
    method: str = 'initialize'
    params: InitializeRequestParams

    class InitializeRequestParams(InitializeParams):
        pass

InitializeRequestParams = InitializeRequest.InitializeRequestParams

class SessionStartRequest(BaseModel):
    method: str = 'session/start'
    params: SessionStartRequestParams

    class SessionStartRequestParams(SessionStartParams):
        pass

SessionStartRequestParams = SessionStartRequest.SessionStartRequestParams

class SessionResumeRequest(BaseModel):
    method: str = 'session/resume'
    params: SessionResumeRequestParams

    class SessionResumeRequestParams(SessionResumeParams):
        pass

SessionResumeRequestParams = SessionResumeRequest.SessionResumeRequestParams

class SessionReadRequest(BaseModel):
    method: str = 'session/read'
    params: SessionReadRequestParams

    class SessionReadRequestParams(SessionReadParams):
        pass

SessionReadRequestParams = SessionReadRequest.SessionReadRequestParams

class SessionArchiveRequest(BaseModel):
    method: str = 'session/archive'
    params: SessionArchiveRequestParams

    class SessionArchiveRequestParams(SessionArchiveParams):
        pass

SessionArchiveRequestParams = SessionArchiveRequest.SessionArchiveRequestParams

class TurnStartRequest(BaseModel):
    method: str = 'turn/start'
    params: TurnStartRequestParams

    class TurnStartRequestParams(TurnStartParams):
        pass

TurnStartRequestParams = TurnStartRequest.TurnStartRequestParams

class ApprovalResolveRequest(BaseModel):
    method: str = 'approval/resolve'
    params: ApprovalResolveRequestParams

    class ApprovalResolveRequestParams(ApprovalResolveParams):
        pass

ApprovalResolveRequestParams = ApprovalResolveRequest.ApprovalResolveRequestParams

class UserInputResolveRequest(BaseModel):
    method: str = 'input/resolveUserInput'
    params: UserInputResolveRequestParams

    class UserInputResolveRequestParams(UserInputResolveParams):
        pass

UserInputResolveRequestParams = UserInputResolveRequest.UserInputResolveRequestParams

class SetModelRequest(BaseModel):
    method: str = 'control/setModel'
    params: SetModelRequestParams

    class SetModelRequestParams(SetModelParams):
        pass

SetModelRequestParams = SetModelRequest.SetModelRequestParams

class SetPermissionModeRequest(BaseModel):
    method: str = 'control/setPermissionMode'
    params: SetPermissionModeRequestParams

    class SetPermissionModeRequestParams(SetPermissionModeParams):
        pass

SetPermissionModeRequestParams = SetPermissionModeRequest.SetPermissionModeRequestParams

class SetThinkingRequest(BaseModel):
    method: str = 'control/setThinking'
    params: SetThinkingRequestParams

    class SetThinkingRequestParams(SetThinkingParams):
        pass

SetThinkingRequestParams = SetThinkingRequest.SetThinkingRequestParams

class StopTaskRequest(BaseModel):
    method: str = 'control/stopTask'
    params: StopTaskRequestParams

    class StopTaskRequestParams(StopTaskParams):
        pass

StopTaskRequestParams = StopTaskRequest.StopTaskRequestParams

class RewindFilesRequest(BaseModel):
    method: str = 'control/rewindFiles'
    params: RewindFilesRequestParams

    class RewindFilesRequestParams(RewindFilesParams):
        pass

RewindFilesRequestParams = RewindFilesRequest.RewindFilesRequestParams

class UpdateEnvRequest(BaseModel):
    method: str = 'control/updateEnv'
    params: UpdateEnvRequestParams

    class UpdateEnvRequestParams(UpdateEnvParams):
        pass

UpdateEnvRequestParams = UpdateEnvRequest.UpdateEnvRequestParams

class CancelRequest(BaseModel):
    method: str = 'control/cancelRequest'
    params: CancelRequestParams

    class CancelRequestParams(CancelRequestParams):
        pass

CancelRequestParams = CancelRequest.CancelRequestParams

class ConfigWriteRequest(BaseModel):
    method: str = 'config/value/write'
    params: ConfigWriteRequestParams

    class ConfigWriteRequestParams(ConfigWriteParams):
        pass

ConfigWriteRequestParams = ConfigWriteRequest.ConfigWriteRequestParams

class HookCallbackResponseRequest(BaseModel):
    method: str = 'hook/callbackResponse'
    params: HookCallbackResponseRequestParams

    class HookCallbackResponseRequestParams(HookCallbackResponseParams):
        pass

HookCallbackResponseRequestParams = HookCallbackResponseRequest.HookCallbackResponseRequestParams

class McpRouteMessageResponseRequest(BaseModel):
    method: str = 'mcp/routeMessageResponse'
    params: McpRouteMessageResponseRequestParams

    class McpRouteMessageResponseRequestParams(McpRouteMessageResponseParams):
        pass

McpRouteMessageResponseRequestParams = McpRouteMessageResponseRequest.McpRouteMessageResponseRequestParams

class McpSetServersRequest(BaseModel):
    method: str = 'mcp/setServers'
    params: McpSetServersRequestParams

    class McpSetServersRequestParams(McpSetServersParams):
        pass

McpSetServersRequestParams = McpSetServersRequest.McpSetServersRequestParams

class McpReconnectRequest(BaseModel):
    method: str = 'mcp/reconnect'
    params: McpReconnectRequestParams

    class McpReconnectRequestParams(McpReconnectParams):
        pass

McpReconnectRequestParams = McpReconnectRequest.McpReconnectRequestParams

class McpToggleRequest(BaseModel):
    method: str = 'mcp/toggle'
    params: McpToggleRequestParams

    class McpToggleRequestParams(McpToggleParams):
        pass

McpToggleRequestParams = McpToggleRequest.McpToggleRequestParams

class ConfigApplyFlagsRequest(BaseModel):
    method: str = 'config/applyFlags'
    params: ConfigApplyFlagsRequestParams

    class ConfigApplyFlagsRequestParams(ConfigApplyFlagsParams):
        pass

ConfigApplyFlagsRequestParams = ConfigApplyFlagsRequest.ConfigApplyFlagsRequestParams


# ---------------------------------------------------------------------------
# Additional types
# ---------------------------------------------------------------------------

class AgentInfo(BaseModel):
    name: str
    description: str | None = None

class ClientHookCallbackResponseParams(BaseModel):
    callback_id: str
    output: Any

class ConfigReadResult(BaseModel):
    config: Any
    sources: dict[str, Any] = {}

class ContextUsageCategory(BaseModel):
    name: str
    tokens: int

class ContextUsageResult(BaseModel):
    categories: list[ContextUsageCategory]
    is_auto_compact_enabled: bool
    max_tokens: int
    model: str
    percentage: float
    raw_max_tokens: int
    total_tokens: int
    auto_compact_threshold: int | None = None
    message_breakdown: MessageBreakdown | None = None

class FileChangeInfo(BaseModel):
    kind: str
    path: str

class HookCallbackMatcher(BaseModel):
    hook_callback_ids: list[str]
    matcher: str | None = None
    timeout: int | None = None

class JsonRpcError(BaseModel):
    code: int
    message: str
    request_id: RequestId
    data: Any = None

class JsonRpcNotification(BaseModel):
    method: str
    params: Any = None

class JsonRpcRequest(BaseModel):
    method: str
    request_id: RequestId
    params: Any = None

class JsonRpcResponse(BaseModel):
    request_id: RequestId
    result: Any = None

class McpServerInit(BaseModel):
    name: str
    status: McpConnectionStatus

class McpServerStatus(BaseModel):
    name: str
    status: McpConnectionStatus
    error: str | None = None
    tool_count: int = 0

class McpSetServersResult(BaseModel):
    added: list[str]
    errors: dict[str, str]
    removed: list[str]

class McpStatusResult(BaseModel):
    mcpServers: list[McpServerStatus]

class MessageBreakdown(BaseModel):
    assistant_message_tokens: int
    attachment_tokens: int
    tool_call_tokens: int
    tool_result_tokens: int
    user_message_tokens: int

class PermissionDenialInfo(BaseModel):
    tool_input: Any
    tool_name: str
    tool_use_id: str

class PermissionRule(BaseModel):
    behavior: PermissionBehavior
    source: PermissionRuleSource
    value: PermissionRuleValue

class PermissionRuleValue(BaseModel):
    tool_pattern: str
    rule_content: str | None = None

class PersistedFileError(BaseModel):
    error: str
    filename: str

class PersistedFileInfo(BaseModel):
    file_id: str
    filename: str

class PluginInit(BaseModel):
    name: str
    path: str
    source: str | None = None

class PluginReloadResult(BaseModel):
    agents: list[str]
    commands: list[str]
    error_count: int
    plugins: list[str]

class ServerAskForApprovalParams(BaseModel):
    input: Any
    request_id: str
    tool_name: str
    tool_use_id: str
    agent_id: str | None = None
    blocked_path: str | None = None
    decision_reason: str | None = None
    description: str | None = None
    display_name: str | None = None
    permission_suggestions: list[Any] | None = None
    title: str | None = None

class ServerHookCallbackParams(BaseModel):
    callback_id: str
    input: Any
    request_id: str
    tool_use_id: str | None = None

class ServerMcpRouteMessageParams(BaseModel):
    message: Any
    request_id: str
    server_name: str

class ServerRequestUserInputParams(BaseModel):
    prompt: str
    request_id: str
    choices: list[str] | None = None
    default: str | None = None
    description: str | None = None

class SessionModelUsage(BaseModel):
    cache_creation_input_tokens: int
    cache_read_input_tokens: int
    context_window: int
    cost_usd: float
    input_tokens: int
    max_output_tokens: int
    output_tokens: int
    web_search_requests: int

class TaskUsage(BaseModel):
    duration_ms: int
    tool_uses: int
    total_tokens: int

class ThinkingLevel(BaseModel):
    effort: ReasoningEffort
    budget_tokens: int | None = None
    options: dict[str, Any] | None = None

class TokenUsage(BaseModel):
    input_tokens: int
    output_tokens: int
    cache_creation_input_tokens: int = 0
    cache_read_input_tokens: int = 0


# ── Compatibility stubs ──
#
# Loose BaseModel subclasses for names referenced by client.py / tests
# but not yet emitted by the coco-rs schema generator. These accept any
# fields (`extra='allow'`) and are regenerated on every run of
# ./coco-sdk/scripts/generate_python.sh.

class AgentDefinitionConfig(BaseModel):
    """Stub for AgentDefinitionConfig pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class AgentMessageDeltaParams(BaseModel):
    """Stub for AgentMessageDeltaParams pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class ApprovalDecision(BaseModel):
    """Stub for ApprovalDecision pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class CommandExecutionItem(BaseModel):
    """Stub for CommandExecutionItem pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class ConfigReadRequest(BaseModel):
    """Stub for ConfigReadRequest pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class HookBehavior(BaseModel):
    """Stub for HookBehavior pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class HookCallbackConfig(BaseModel):
    """Stub for HookCallbackConfig pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class HookCallbackOutput(BaseModel):
    """Stub for HookCallbackOutput pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class ItemStatus(BaseModel):
    """Stub for ItemStatus pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class KeepAliveRequest(BaseModel):
    """Stub for KeepAliveRequest pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class McpServerConfig(BaseModel):
    """Stub for McpServerConfig pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class PostToolUseHookInput(BaseModel):
    """Stub for PostToolUseHookInput pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class PreToolUseHookInput(BaseModel):
    """Stub for PreToolUseHookInput pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class SessionEndedReason(BaseModel):
    """Stub for SessionEndedReason pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class SessionListRequest(BaseModel):
    """Stub for SessionListRequest pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class TurnInterruptRequest(BaseModel):
    """Stub for TurnInterruptRequest pending coco-rs schema emission."""
    model_config = {"extra": "allow"}


class Usage(BaseModel):
    """Stub for Usage pending coco-rs schema emission."""
    model_config = {"extra": "allow"}

