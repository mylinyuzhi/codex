"""Generated protocol types for the cocode SDK.

These types mirror the Rust `cocode-app-server-protocol` crate.
Regenerate with: `scripts/generate_python.sh`

Source schemas: cocode-rs/app-server-protocol/schema/json/

DO NOT EDIT MANUALLY — changes will be overwritten by the generator.
"""

from __future__ import annotations

from enum import Enum
from typing import Any

from pydantic import BaseModel, Field

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

class Usage(BaseModel):
    cache_creation_tokens: int | None = None
    cache_read_tokens: int | None = None
    input_tokens: int = 0
    output_tokens: int = 0
    reasoning_tokens: int | None = None

# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------

class AgentIsolationMode(str, Enum):
    none = 'none'
    worktree = 'worktree'

class AgentMemoryScope(str, Enum):
    user = 'user'
    project = 'project'
    local = 'local'

class ApprovalDecision(str, Enum):
    approve = 'approve'
    approve_session = 'approve_session'
    deny = 'deny'

class ConfigWriteScope(str, Enum):
    user = 'user'
    project = 'project'

class FileChangeKind(str, Enum):
    add = 'add'
    delete = 'delete'
    update = 'update'

class HookBehavior(str, Enum):
    allow = 'allow'
    deny = 'deny'
    error = 'error'

class ItemStatus(str, Enum):
    in_progress = 'in_progress'
    completed = 'completed'
    failed = 'failed'
    declined = 'declined'

class SandboxMode(str, Enum):
    none = 'none'
    read_only = 'read_only'
    strict = 'strict'

class ThinkingMode(str, Enum):
    adaptive = 'adaptive'
    enabled = 'enabled'
    disabled = 'disabled'


# ---------------------------------------------------------------------------
# Item types
# ---------------------------------------------------------------------------

class AgentMessageItem(BaseModel):
    text: str

class ReasoningItem(BaseModel):
    text: str

class CommandExecutionItem(BaseModel):
    aggregated_output: str
    command: str
    status: ItemStatus
    exit_code: int | None = None

class FileChange(BaseModel):
    kind: FileChangeKind
    path: str

class FileChangeItem(BaseModel):
    changes: list[FileChange]
    status: ItemStatus

class McpToolCallResult(BaseModel):
    content: list[Any]
    structured_content: Any = None

class McpToolCallError(BaseModel):
    message: str

class McpToolCallItem(BaseModel):
    server: str
    status: ItemStatus
    tool: str
    arguments: Any = None
    error: McpToolCallError | None = None
    result: McpToolCallResult | None = None

class WebSearchItem(BaseModel):
    query: str
    status: ItemStatus

class SubagentItem(BaseModel):
    agent_id: str
    agent_type: str
    description: str
    status: ItemStatus
    is_background: bool = False
    result: str | None = None

class GenericToolCallItem(BaseModel):
    status: ItemStatus
    tool: str
    input: Any = None
    is_error: bool = False
    output: str | None = None

class ErrorItem(BaseModel):
    message: str


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
    session_id: str
    commands: list[CommandInfo] | None = None
    models: list[str] | None = None
    protocol_version: str = '1'

class SessionResultParams(BaseModel):
    duration_ms: int
    session_id: str
    stop_reason: SessionEndedReason
    total_turns: int
    usage: Usage
    duration_api_ms: int | None = None
    structured_output: Any = None
    total_cost_cents: int | None = None

class SessionEndedParams(BaseModel):
    reason: SessionEndedReason

class TurnStartedParams(BaseModel):
    turn_id: str
    turn_number: int

class TurnCompletedParams(BaseModel):
    turn_id: str
    usage: Usage

class TurnFailedParams(BaseModel):
    error: str

class TurnInterruptedNotifParams(BaseModel):
    turn_id: str | None = None

class MaxTurnsReachedParams(BaseModel):
    max_turns: int | None = None

class TurnRetryParams(BaseModel):
    attempt: int
    delay_ms: int
    max_attempts: int

class ItemEventParams(BaseModel):
    item: ThreadItem

class AgentMessageDeltaParams(BaseModel):
    delta: str
    item_id: str
    turn_id: str

class ReasoningDeltaParams(BaseModel):
    delta: str
    item_id: str
    turn_id: str

class SubagentSpawnedParams(BaseModel):
    agent_id: str
    agent_type: str
    description: str
    color: str | None = None

class SubagentCompletedParams(BaseModel):
    agent_id: str
    result: str

class SubagentBackgroundedParams(BaseModel):
    agent_id: str
    output_file: str

class SubagentProgressParams(BaseModel):
    agent_id: str
    message: str | None = None

class McpStartupStatusParams(BaseModel):
    server: str
    status: str

class McpStartupCompleteParams(BaseModel):
    failed: list[McpServerFailure]
    servers: list[McpServerInfoParams]

class ContextCompactedParams(BaseModel):
    removed_messages: int
    summary_tokens: int

class ContextUsageWarningParams(BaseModel):
    estimated_tokens: int
    percent_left: float
    warning_threshold: int

class CompactionStartedParams(BaseModel):
    pass

class CompactionFailedParams(BaseModel):
    attempts: int
    error: str

class ContextClearedParams(BaseModel):
    new_mode: str

class TaskStartedParams(BaseModel):
    task_id: str
    task_type: str

class TaskCompletedParams(BaseModel):
    result: str
    task_id: str
    is_error: bool = False

class TaskProgressParams(BaseModel):
    task_id: str
    message: str | None = None

class AgentsKilledParams(BaseModel):
    agent_ids: list[str]
    count: int

class ModelFallbackStartedParams(BaseModel):
    from_model: str
    reason: str
    to_model: str

class ModelFallbackCompletedParams(BaseModel):
    pass

class PermissionModeChangedParams(BaseModel):
    mode: str

class PromptSuggestionParams(BaseModel):
    suggestions: list[str]

class ErrorNotificationParams(BaseModel):
    message: str
    category: ErrorCategory | None = None
    error_info: ErrorInfo | None = None
    retryable: bool = False

class RateLimitParams(BaseModel):
    limit: int | None = None
    provider: str | None = None
    remaining: int | None = None
    reset_at: int | None = None

class KeepAliveNotifParams(BaseModel):
    timestamp: int

class IdeSelectionChangedParams(BaseModel):
    end_line: int
    file_path: str
    start_line: int
    selected_text: str = ''

class IdeDiagnosticsUpdatedParams(BaseModel):
    diagnostics: list[IdeDiagnosticInfo]
    file_path: str
    new_count: int

class PlanModeChangedParams(BaseModel):
    entered: bool
    approved: bool | None = None
    plan_file: str | None = None

class QueueStateChangedParams(BaseModel):
    queued: int

class CommandQueuedParams(BaseModel):
    id: str
    preview: str

class CommandDequeuedParams(BaseModel):
    id: str

class RewindCompletedParams(BaseModel):
    messages_removed: int
    restored_files: int
    rewound_turn: int

class RewindFailedParams(BaseModel):
    error: str

class CostWarningParams(BaseModel):
    current_cost_cents: int
    threshold_cents: int
    budget_cents: int | None = None

class SandboxStateChangedParams(BaseModel):
    active: bool
    enforcement: str

class SandboxViolationsDetectedParams(BaseModel):
    count: int

class FastModeChangedParams(BaseModel):
    active: bool

class AgentsRegisteredParams(BaseModel):
    agents: list[AgentInfo]

class HookExecutedParams(BaseModel):
    hook_name: str
    hook_type: str

class SummarizeCompletedParams(BaseModel):
    from_turn: int
    summary_tokens: int

class SummarizeFailedParams(BaseModel):
    error: str

class StreamStallDetectedParams(BaseModel):
    turn_id: str | None = None

class StreamWatchdogWarningParams(BaseModel):
    elapsed_secs: int

class StreamRequestEndParams(BaseModel):
    usage: Usage


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


# ---------------------------------------------------------------------------
# Server requests (server -> client, require response)
# ---------------------------------------------------------------------------

class AskForApprovalParams(BaseModel):
    input: Any
    request_id: str
    tool_name: str
    blocked_path: str | None = None
    decision_reason: str | None = None
    description: str | None = None
    permission_suggestions: list[PermissionSuggestion] | None = None

class RequestUserInputParams(BaseModel):
    message: str
    request_id: str
    questions: Any = None

class McpRouteMessageParams(BaseModel):
    message: Any
    request_id: str
    server_name: str

class HookCallbackParams(BaseModel):
    callback_id: str
    event_type: str
    request_id: str
    input: Any = None

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

class AgentHookConfig(BaseModel):
    command: str
    event: str
    matcher: str | None = None
    timeout: int | None = None

class AgentDefinitionConfig(BaseModel):
    background: bool = False
    color: str | None = None
    critical_reminder: str | None = None
    description: str | None = None
    disallowed_tools: list[str] | None = None
    fork_context: bool = False
    hooks: list[AgentHookConfig] | None = None
    isolation: AgentIsolationMode | None = None
    max_turns: int | None = None
    mcp_servers: list[str] | None = None
    memory: AgentMemoryScope | None = None
    model: str | None = None
    permission_mode: str | None = None
    prompt: str | None = None
    skills: list[str] | None = None
    tools: list[str] | None = None
    use_custom_prompt: bool = False

class HookCallbackConfig(BaseModel):
    callback_id: str
    event: str
    matcher: str | None = None
    timeout_ms: int | None = None

class SandboxConfig(BaseModel):
    auto_allow_bash_if_sandboxed: bool = False
    exclude_commands: list[str] | None = None
    mode: SandboxMode = 'none'
    network_access: bool = False

class ThinkingConfig(BaseModel):
    mode: ThinkingMode
    max_tokens: int | None = None

class OutputFormatConfig(BaseModel):
    schema_: Any = Field(alias='schema')

class HookMatcherConfig(BaseModel):
    command: str
    args: list[str] = []
    tool_name: str | None = None

class CommandInfo(BaseModel):
    name: str
    description: str | None = None

class ClientInfo(BaseModel):
    name: str
    title: str | None = None
    version: str | None = None

class InitializeCapabilities(BaseModel):
    experimental_api: bool = False
    opt_out_notification_methods: list[str] | None = None

class SessionSummary(BaseModel):
    id: str
    created_at: str | None = None
    model: str | None = None
    name: str | None = None
    turn_count: int = 0
    updated_at: str | None = None
    working_dir: str | None = None

# Union type: see Rust source for variants
SystemPromptConfig = Any

# Union type: see Rust source for variants
ToolsConfig = Any

# Union type: see Rust source for variants
ErrorInfo = Any


# ---------------------------------------------------------------------------
# Hook input/output types
# ---------------------------------------------------------------------------

class PreToolUseHookInput(BaseModel):
    tool_name: str
    tool_input: Any = None
    tool_use_id: str | None = None

class PostToolUseHookInput(BaseModel):
    tool_name: str
    is_error: bool = False
    tool_input: Any = None
    tool_output: str | None = None
    tool_use_id: str | None = None

class PostToolUseFailureHookInput(BaseModel):
    error: str
    tool_name: str
    is_interrupt: bool = False
    tool_input: Any = None
    tool_use_id: str | None = None

class HookCallbackOutput(BaseModel):
    behavior: HookBehavior
    message: str | None = None
    updated_input: Any = None

class StopHookInput(BaseModel):
    stop_reason: str

class SubagentStartHookInput(BaseModel):
    agent_type: str
    prompt: str
    agent_id: str | None = None

class SubagentStopHookInput(BaseModel):
    agent_id: str
    agent_type: str
    output: str | None = None

class UserPromptSubmitHookInput(BaseModel):
    prompt: str

class NotificationHookInput(BaseModel):
    notification_type: str
    payload: Any = None

class PreCompactHookInput(BaseModel):
    trigger: str
    custom_instructions: str | None = None

class PermissionRequestHookInput(BaseModel):
    tool_name: str
    permission_suggestions: Any = None
    tool_input: Any = None

class SessionStartHookInput(BaseModel):
    session_id: str

class SessionEndHookInput(BaseModel):
    reason: str
    session_id: str


# ---------------------------------------------------------------------------
# Client request params
# ---------------------------------------------------------------------------

class InitializeRequestParams(BaseModel):
    capabilities: InitializeCapabilities | None = None
    client_info: ClientInfo | None = None

class SessionStartRequestParams(BaseModel):
    prompt: str
    agents: dict[str, AgentDefinitionConfig] | None = None
    cwd: str | None = None
    disable_builtin_agents: bool | None = None
    env: dict[str, str] | None = None
    hooks: list[HookCallbackConfig] | None = None
    max_budget_cents: int | None = None
    max_turns: int | None = None
    mcp_servers: dict[str, McpServerConfig] | None = None
    model: str | None = None
    output_format: OutputFormatConfig | None = None
    permission_mode: str | None = None
    permission_rules: list[Any] | None = None
    sandbox: SandboxConfig | None = None
    system_prompt: SystemPromptConfig | None = None
    system_prompt_suffix: str | None = None
    thinking: ThinkingConfig | None = None
    tools: ToolsConfig | None = None

class SessionResumeRequestParams(BaseModel):
    session_id: str
    prompt: str | None = None

class TurnStartRequestParams(BaseModel):
    text: str

class TurnInterruptRequestParams(BaseModel):
    turn_id: str | None = None

class ApprovalResolveRequestParams(BaseModel):
    decision: ApprovalDecision
    request_id: str

class UserInputResolveRequestParams(BaseModel):
    request_id: str
    response: Any

class SetModelRequestParams(BaseModel):
    model: str

class SetPermissionModeRequestParams(BaseModel):
    mode: str

class StopTaskRequestParams(BaseModel):
    task_id: str

class HookCallbackResponseParams(BaseModel):
    request_id: str
    error: str | None = None
    output: Any = None

class SetThinkingRequestParams(BaseModel):
    thinking: ThinkingConfig

class RewindFilesRequestParams(BaseModel):
    turn_id: str

class UpdateEnvRequestParams(BaseModel):
    env: dict[str, str]

class KeepAliveRequestParams(BaseModel):
    timestamp: int | None = None

class SessionListRequestParams(BaseModel):
    cursor: str | None = None
    limit: int | None = None

class SessionReadRequestParams(BaseModel):
    session_id: str

class SessionArchiveRequestParams(BaseModel):
    session_id: str

class ConfigReadRequestParams(BaseModel):
    key: str | None = None

class ConfigWriteRequestParams(BaseModel):
    key: str
    value: Any
    scope: ConfigWriteScope = 'user'

class McpRouteMessageResponseParams(BaseModel):
    request_id: str
    error: str | None = None
    response: Any = None

class CancelRequestParams(BaseModel):
    request_id: str


# ---------------------------------------------------------------------------
# Client request wrappers
# ---------------------------------------------------------------------------

class InitializeRequest(BaseModel):
    method: str = 'initialize'
    params: InitializeRequestParams

    class InitializeRequestParams(InitializeRequestParams):
        pass

InitializeRequestParams = InitializeRequest.InitializeRequestParams

class SessionStartRequest(BaseModel):
    method: str = 'session/start'
    params: SessionStartRequestParams

    class SessionStartRequestParams(SessionStartRequestParams):
        pass

SessionStartRequestParams = SessionStartRequest.SessionStartRequestParams

class SessionResumeRequest(BaseModel):
    method: str = 'session/resume'
    params: SessionResumeRequestParams

    class SessionResumeRequestParams(SessionResumeRequestParams):
        pass

SessionResumeRequestParams = SessionResumeRequest.SessionResumeRequestParams

class TurnStartRequest(BaseModel):
    method: str = 'turn/start'
    params: TurnStartRequestParams

    class TurnStartRequestParams(TurnStartRequestParams):
        pass

TurnStartRequestParams = TurnStartRequest.TurnStartRequestParams

class TurnInterruptRequest(BaseModel):
    method: str = 'turn/interrupt'
    params: TurnInterruptRequestParams

    class TurnInterruptRequestParams(TurnInterruptRequestParams):
        pass

TurnInterruptRequestParams = TurnInterruptRequest.TurnInterruptRequestParams

class ApprovalResolveRequest(BaseModel):
    method: str = 'approval/resolve'
    params: ApprovalResolveRequestParams

    class ApprovalResolveRequestParams(ApprovalResolveRequestParams):
        pass

ApprovalResolveRequestParams = ApprovalResolveRequest.ApprovalResolveRequestParams

class UserInputResolveRequest(BaseModel):
    method: str = 'input/resolveUserInput'
    params: UserInputResolveRequestParams

    class UserInputResolveRequestParams(UserInputResolveRequestParams):
        pass

UserInputResolveRequestParams = UserInputResolveRequest.UserInputResolveRequestParams

class SetModelRequest(BaseModel):
    method: str = 'control/setModel'
    params: SetModelRequestParams

    class SetModelRequestParams(SetModelRequestParams):
        pass

SetModelRequestParams = SetModelRequest.SetModelRequestParams

class SetPermissionModeRequest(BaseModel):
    method: str = 'control/setPermissionMode'
    params: SetPermissionModeRequestParams

    class SetPermissionModeRequestParams(SetPermissionModeRequestParams):
        pass

SetPermissionModeRequestParams = SetPermissionModeRequest.SetPermissionModeRequestParams

class StopTaskRequest(BaseModel):
    method: str = 'control/stopTask'
    params: StopTaskRequestParams

    class StopTaskRequestParams(StopTaskRequestParams):
        pass

StopTaskRequestParams = StopTaskRequest.StopTaskRequestParams

class HookCallbackResponseRequest(BaseModel):
    method: str = 'hook/callbackResponse'
    params: HookCallbackResponseRequestParams

    class HookCallbackResponseRequestParams(HookCallbackResponseParams):
        pass

HookCallbackResponseRequestParams = HookCallbackResponseRequest.HookCallbackResponseRequestParams

class SetThinkingRequest(BaseModel):
    method: str = 'control/setThinking'
    params: SetThinkingRequestParams

    class SetThinkingRequestParams(SetThinkingRequestParams):
        pass

SetThinkingRequestParams = SetThinkingRequest.SetThinkingRequestParams

class RewindFilesRequest(BaseModel):
    method: str = 'control/rewindFiles'
    params: RewindFilesRequestParams

    class RewindFilesRequestParams(RewindFilesRequestParams):
        pass

RewindFilesRequestParams = RewindFilesRequest.RewindFilesRequestParams

class UpdateEnvRequest(BaseModel):
    method: str = 'control/updateEnv'
    params: UpdateEnvRequestParams

    class UpdateEnvRequestParams(UpdateEnvRequestParams):
        pass

UpdateEnvRequestParams = UpdateEnvRequest.UpdateEnvRequestParams

class KeepAliveRequest(BaseModel):
    method: str = 'control/keepAlive'
    params: KeepAliveRequestParams

    class KeepAliveRequestParams(KeepAliveRequestParams):
        pass

KeepAliveRequestParams = KeepAliveRequest.KeepAliveRequestParams

class SessionListRequest(BaseModel):
    method: str = 'session/list'
    params: SessionListRequestParams

    class SessionListRequestParams(SessionListRequestParams):
        pass

SessionListRequestParams = SessionListRequest.SessionListRequestParams

class SessionReadRequest(BaseModel):
    method: str = 'session/read'
    params: SessionReadRequestParams

    class SessionReadRequestParams(SessionReadRequestParams):
        pass

SessionReadRequestParams = SessionReadRequest.SessionReadRequestParams

class SessionArchiveRequest(BaseModel):
    method: str = 'session/archive'
    params: SessionArchiveRequestParams

    class SessionArchiveRequestParams(SessionArchiveRequestParams):
        pass

SessionArchiveRequestParams = SessionArchiveRequest.SessionArchiveRequestParams

class ConfigReadRequest(BaseModel):
    method: str = 'config/read'
    params: ConfigReadRequestParams

    class ConfigReadRequestParams(ConfigReadRequestParams):
        pass

ConfigReadRequestParams = ConfigReadRequest.ConfigReadRequestParams

class ConfigWriteRequest(BaseModel):
    method: str = 'config/value/write'
    params: ConfigWriteRequestParams

    class ConfigWriteRequestParams(ConfigWriteRequestParams):
        pass

ConfigWriteRequestParams = ConfigWriteRequest.ConfigWriteRequestParams

class McpRouteMessageResponseRequest(BaseModel):
    method: str = 'mcp/routeMessageResponse'
    params: McpRouteMessageResponseRequestParams

    class McpRouteMessageResponseRequestParams(McpRouteMessageResponseParams):
        pass

McpRouteMessageResponseRequestParams = McpRouteMessageResponseRequest.McpRouteMessageResponseRequestParams

class CancelRequest(BaseModel):
    method: str = 'control/cancelRequest'
    params: CancelRequestParams

    class CancelRequestParams(CancelRequestParams):
        pass

CancelRequestParams = CancelRequest.CancelRequestParams


# ---------------------------------------------------------------------------
# Additional types
# ---------------------------------------------------------------------------

class AgentInfo(BaseModel):
    agent_type: str
    name: str
    description: str | None = None

class ConfigReadResult(BaseModel):
    config: Any

class IdeDiagnosticInfo(BaseModel):
    line: int
    message: str
    severity: str

class InitializeResult(BaseModel):
    platform_family: str
    platform_os: str
    protocol_version: str

class JsonRpcError(BaseModel):
    error: JsonRpcErrorData
    id: RequestId

class JsonRpcNotification(BaseModel):
    method: str
    params: Any = None

class JsonRpcRequest(BaseModel):
    id: RequestId
    method: str
    params: Any = None

class JsonRpcResponse(BaseModel):
    id: RequestId
    result: Any

class McpServerFailure(BaseModel):
    error: str
    name: str

class McpServerInfoParams(BaseModel):
    name: str
    tool_count: int

class PermissionSuggestion(BaseModel):
    behavior: str
    reason: str | None = None

class SdkMcpToolDef(BaseModel):
    name: str
    description: str | None = None
    input_schema: Any = None

class SessionListResult(BaseModel):
    sessions: list[SessionSummary]
    next_cursor: str | None = None
