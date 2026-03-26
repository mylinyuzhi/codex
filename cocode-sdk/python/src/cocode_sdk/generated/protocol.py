"""Generated protocol types for the cocode SDK.

These types mirror the Rust `cocode-app-server-protocol` crate.
Regenerate with: `scripts/generate_python.sh`

Source schemas: cocode-rs/app-server-protocol/schema/json/
"""

from __future__ import annotations

from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------


class Usage(BaseModel):
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int | None = None
    cache_creation_tokens: int | None = None
    reasoning_tokens: int | None = None


# ---------------------------------------------------------------------------
# Item types
# ---------------------------------------------------------------------------


class ItemStatus(str, Enum):
    in_progress = "in_progress"
    completed = "completed"
    failed = "failed"
    declined = "declined"


class FileChangeKind(str, Enum):
    add = "add"
    delete = "delete"
    update = "update"


class FileChange(BaseModel):
    path: str
    kind: FileChangeKind


class AgentMessageItem(BaseModel):
    text: str


class ReasoningItem(BaseModel):
    text: str


class CommandExecutionItem(BaseModel):
    command: str
    aggregated_output: str = ""
    exit_code: int | None = None
    status: ItemStatus = ItemStatus.in_progress


class FileChangeItem(BaseModel):
    changes: list[FileChange] = []
    status: ItemStatus = ItemStatus.in_progress


class McpToolCallResult(BaseModel):
    content: list[Any] = []
    structured_content: Any | None = None


class McpToolCallError(BaseModel):
    message: str


class McpToolCallItem(BaseModel):
    server: str
    tool: str
    arguments: Any = None
    result: McpToolCallResult | None = None
    error: McpToolCallError | None = None
    status: ItemStatus = ItemStatus.in_progress


class WebSearchItem(BaseModel):
    query: str
    status: ItemStatus = ItemStatus.in_progress


class SubagentItem(BaseModel):
    agent_id: str
    agent_type: str
    description: str
    is_background: bool = False
    result: str | None = None
    status: ItemStatus = ItemStatus.in_progress


class GenericToolCallItem(BaseModel):
    tool: str
    input: Any = None
    output: str | None = None
    is_error: bool = False
    status: ItemStatus = ItemStatus.in_progress


class ErrorItem(BaseModel):
    message: str


class ThreadItem(BaseModel):
    """A discrete operation within a turn."""

    id: str
    type: str
    # Flattened fields from the specific item type
    # Use model_extra to capture all additional fields
    model_config = {"extra": "allow"}

    def as_agent_message(self) -> AgentMessageItem | None:
        if self.type == "agent_message":
            return AgentMessageItem.model_validate(self.model_extra or {})
        return None

    def as_command_execution(self) -> CommandExecutionItem | None:
        if self.type == "command_execution":
            return CommandExecutionItem.model_validate(self.model_extra or {})
        return None

    def as_file_change(self) -> FileChangeItem | None:
        if self.type == "file_change":
            return FileChangeItem.model_validate(self.model_extra or {})
        return None

    def as_mcp_tool_call(self) -> McpToolCallItem | None:
        if self.type == "mcp_tool_call":
            return McpToolCallItem.model_validate(self.model_extra or {})
        return None

    def as_subagent(self) -> SubagentItem | None:
        if self.type == "subagent":
            return SubagentItem.model_validate(self.model_extra or {})
        return None

    def as_tool_call(self) -> GenericToolCallItem | None:
        if self.type == "tool_call":
            return GenericToolCallItem.model_validate(self.model_extra or {})
        return None

    def as_reasoning(self) -> ReasoningItem | None:
        if self.type == "reasoning":
            return ReasoningItem.model_validate(self.model_extra or {})
        return None

    def as_web_search(self) -> WebSearchItem | None:
        if self.type == "web_search":
            return WebSearchItem.model_validate(self.model_extra or {})
        return None

    def as_error(self) -> ErrorItem | None:
        if self.type == "error":
            return ErrorItem.model_validate(self.model_extra or {})
        return None


# ---------------------------------------------------------------------------
# Server notification params
# ---------------------------------------------------------------------------


class SessionStartedParams(BaseModel):
    session_id: str
    protocol_version: str = "1"
    models: list[str] | None = None
    commands: list[Any] | None = None


class TurnStartedParams(BaseModel):
    turn_id: str
    turn_number: int


class TurnCompletedParams(BaseModel):
    turn_id: str
    usage: Usage


class TurnFailedParams(BaseModel):
    error: str


class ItemEventParams(BaseModel):
    item: ThreadItem


class AgentMessageDeltaParams(BaseModel):
    item_id: str
    turn_id: str
    delta: str


class ReasoningDeltaParams(BaseModel):
    item_id: str
    turn_id: str
    delta: str


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


class McpServerInfoParams(BaseModel):
    name: str
    tool_count: int


class McpServerFailure(BaseModel):
    name: str
    error: str


class McpStartupStatusParams(BaseModel):
    server: str
    status: str


class McpStartupCompleteParams(BaseModel):
    servers: list[McpServerInfoParams] = []
    failed: list[McpServerFailure] = []


class ContextCompactedParams(BaseModel):
    removed_messages: int
    summary_tokens: int


class ContextUsageWarningParams(BaseModel):
    estimated_tokens: int
    warning_threshold: int
    percent_left: float


class ErrorNotificationParams(BaseModel):
    message: str
    category: str | None = None
    retryable: bool = False


class RateLimitParams(BaseModel):
    info: Any


# ---------------------------------------------------------------------------
# Server notifications (tagged union)
# ---------------------------------------------------------------------------


class ServerNotification(BaseModel):
    """An event from the server. Use `method` to determine the event type."""

    method: str
    params: dict[str, Any] = {}

    def as_session_started(self) -> SessionStartedParams | None:
        if self.method == "session/started":
            return SessionStartedParams.model_validate(self.params)
        return None

    def as_turn_started(self) -> TurnStartedParams | None:
        if self.method == "turn/started":
            return TurnStartedParams.model_validate(self.params)
        return None

    def as_turn_completed(self) -> TurnCompletedParams | None:
        if self.method == "turn/completed":
            return TurnCompletedParams.model_validate(self.params)
        return None

    def as_turn_failed(self) -> TurnFailedParams | None:
        if self.method == "turn/failed":
            return TurnFailedParams.model_validate(self.params)
        return None

    def as_item_started(self) -> ItemEventParams | None:
        if self.method == "item/started":
            return ItemEventParams.model_validate(self.params)
        return None

    def as_item_updated(self) -> ItemEventParams | None:
        if self.method == "item/updated":
            return ItemEventParams.model_validate(self.params)
        return None

    def as_item_completed(self) -> ItemEventParams | None:
        if self.method == "item/completed":
            return ItemEventParams.model_validate(self.params)
        return None

    def as_agent_message_delta(self) -> AgentMessageDeltaParams | None:
        if self.method == "agentMessage/delta":
            return AgentMessageDeltaParams.model_validate(self.params)
        return None

    def as_error(self) -> ErrorNotificationParams | None:
        if self.method == "error":
            return ErrorNotificationParams.model_validate(self.params)
        return None

    def as_reasoning_delta(self) -> ReasoningDeltaParams | None:
        if self.method == "reasoning/delta":
            return ReasoningDeltaParams.model_validate(self.params)
        return None

    def as_subagent_spawned(self) -> SubagentSpawnedParams | None:
        if self.method == "subagent/spawned":
            return SubagentSpawnedParams.model_validate(self.params)
        return None

    def as_subagent_completed(self) -> SubagentCompletedParams | None:
        if self.method == "subagent/completed":
            return SubagentCompletedParams.model_validate(self.params)
        return None

    def as_subagent_backgrounded(self) -> SubagentBackgroundedParams | None:
        if self.method == "subagent/backgrounded":
            return SubagentBackgroundedParams.model_validate(self.params)
        return None

    def as_mcp_startup_complete(self) -> McpStartupCompleteParams | None:
        if self.method == "mcp/startupComplete":
            return McpStartupCompleteParams.model_validate(self.params)
        return None

    def as_context_compacted(self) -> ContextCompactedParams | None:
        if self.method == "context/compacted":
            return ContextCompactedParams.model_validate(self.params)
        return None

    def as_context_usage_warning(self) -> ContextUsageWarningParams | None:
        if self.method == "context/usageWarning":
            return ContextUsageWarningParams.model_validate(self.params)
        return None

    def as_rate_limit(self) -> RateLimitParams | None:
        if self.method == "rateLimit":
            return RateLimitParams.model_validate(self.params)
        return None

    # ── Phase 2 notification accessors ──────────────────────────────

    def as_task_started(self) -> TaskStartedParams | None:
        if self.method == "task/started":
            return TaskStartedParams.model_validate(self.params)
        return None

    def as_task_completed(self) -> TaskCompletedParams | None:
        if self.method == "task/completed":
            return TaskCompletedParams.model_validate(self.params)
        return None

    def as_turn_interrupted(self) -> TurnInterruptedNotifParams | None:
        if self.method == "turn/interrupted":
            return TurnInterruptedNotifParams.model_validate(self.params)
        return None

    def as_max_turns_reached(self) -> MaxTurnsReachedParams | None:
        if self.method == "turn/maxReached":
            return MaxTurnsReachedParams.model_validate(self.params)
        return None

    def as_model_fallback_started(self) -> ModelFallbackStartedParams | None:
        if self.method == "model/fallbackStarted":
            return ModelFallbackStartedParams.model_validate(self.params)
        return None

    def as_permission_mode_changed(self) -> PermissionModeChangedParams | None:
        if self.method == "permission/modeChanged":
            return PermissionModeChangedParams.model_validate(self.params)
        return None

    def as_mcp_startup_status(self) -> McpStartupStatusParams | None:
        if self.method == "mcp/startupStatus":
            return McpStartupStatusParams.model_validate(self.params)
        return None

    def as_keep_alive(self) -> KeepAliveNotifParams | None:
        if self.method == "keepAlive":
            return KeepAliveNotifParams.model_validate(self.params)
        return None

    def as_session_ended(self) -> SessionEndedParams | None:
        if self.method == "session/ended":
            return SessionEndedParams.model_validate(self.params)
        return None

    def as_session_result(self) -> SessionResultParams | None:
        if self.method == "session/result":
            return SessionResultParams.model_validate(self.params)
        return None

    def as_prompt_suggestion(self) -> PromptSuggestionParams | None:
        if self.method == "prompt/suggestion":
            return PromptSuggestionParams.model_validate(self.params)
        return None


# ---------------------------------------------------------------------------
# Client requests
# ---------------------------------------------------------------------------


class ApprovalDecision(str, Enum):
    approve = "approve"
    approve_session = "approve_session"
    deny = "deny"


class AgentIsolationMode(str, Enum):
    none = "none"
    worktree = "worktree"


class AgentMemoryScope(str, Enum):
    user = "user"
    project = "project"
    local = "local"


class AgentHookConfig(BaseModel):
    event: str
    matcher: str | None = None
    command: str
    timeout: int | None = None


class AgentDefinitionConfig(BaseModel):
    description: str | None = None
    prompt: str | None = None
    tools: list[str] | None = None
    disallowed_tools: list[str] | None = None
    model: str | None = None
    max_turns: int | None = None
    background: bool = False
    isolation: AgentIsolationMode | None = None
    memory: AgentMemoryScope | None = None
    skills: list[str] | None = None
    mcp_servers: list[str] | None = None
    hooks: list[AgentHookConfig] | None = None
    critical_reminder: str | None = None
    use_custom_prompt: bool = False
    color: str | None = None
    permission_mode: str | None = None
    fork_context: bool = False


class HookCallbackConfig(BaseModel):
    callback_id: str
    event: str
    matcher: str | None = None
    timeout_ms: int | None = None


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


class SessionStartRequest(BaseModel):
    method: str = "session/start"
    params: SessionStartRequestParams

    class SessionStartRequestParams(BaseModel):
        prompt: str
        model: str | None = None
        max_turns: int | None = None
        cwd: str | None = None
        system_prompt_suffix: str | None = None
        system_prompt: Any | None = None
        permission_mode: str | None = None
        env: dict[str, str] | None = None
        agents: dict[str, AgentDefinitionConfig] | None = None
        mcp_servers: dict[str, McpServerConfig] | None = None
        output_format: Any | None = None
        sandbox: Any | None = None
        thinking: Any | None = None
        tools: Any | None = None
        permission_rules: list[Any] | None = None
        hooks: list[HookCallbackConfig] | None = None
        max_budget_cents: int | None = None
        disable_builtin_agents: bool | None = None


# Re-export the params class at module level
SessionStartRequestParams = SessionStartRequest.SessionStartRequestParams


class TurnStartRequest(BaseModel):
    method: str = "turn/start"
    params: TurnStartRequestParams

    class TurnStartRequestParams(BaseModel):
        text: str


TurnStartRequestParams = TurnStartRequest.TurnStartRequestParams


class ApprovalResolveRequest(BaseModel):
    method: str = "approval/resolve"
    params: ApprovalResolveRequestParams

    class ApprovalResolveRequestParams(BaseModel):
        request_id: str
        decision: ApprovalDecision


ApprovalResolveRequestParams = ApprovalResolveRequest.ApprovalResolveRequestParams


class UserInputResolveRequest(BaseModel):
    method: str = "input/resolveUserInput"
    params: UserInputResolveRequestParams

    class UserInputResolveRequestParams(BaseModel):
        request_id: str
        response: Any


UserInputResolveRequestParams = UserInputResolveRequest.UserInputResolveRequestParams


class TurnInterruptRequest(BaseModel):
    method: str = "turn/interrupt"
    params: TurnInterruptRequestParams

    class TurnInterruptRequestParams(BaseModel):
        turn_id: str | None = None


TurnInterruptRequestParams = TurnInterruptRequest.TurnInterruptRequestParams


class SetModelRequest(BaseModel):
    method: str = "control/setModel"
    params: SetModelRequestParams

    class SetModelRequestParams(BaseModel):
        model: str


SetModelRequestParams = SetModelRequest.SetModelRequestParams


class SetPermissionModeRequest(BaseModel):
    method: str = "control/setPermissionMode"
    params: SetPermissionModeRequestParams

    class SetPermissionModeRequestParams(BaseModel):
        mode: str


SetPermissionModeRequestParams = SetPermissionModeRequest.SetPermissionModeRequestParams


class StopTaskRequest(BaseModel):
    method: str = "control/stopTask"
    params: StopTaskRequestParams

    class StopTaskRequestParams(BaseModel):
        task_id: str


StopTaskRequestParams = StopTaskRequest.StopTaskRequestParams


class UpdateEnvRequest(BaseModel):
    method: str = "control/updateEnv"
    params: UpdateEnvRequestParams

    class UpdateEnvRequestParams(BaseModel):
        env: dict[str, str]


UpdateEnvRequestParams = UpdateEnvRequest.UpdateEnvRequestParams


class KeepAliveRequest(BaseModel):
    method: str = "control/keepAlive"
    params: KeepAliveRequestParams

    class KeepAliveRequestParams(BaseModel):
        timestamp: int | None = None


KeepAliveRequestParams = KeepAliveRequest.KeepAliveRequestParams


class SetThinkingRequest(BaseModel):
    method: str = "control/setThinking"
    params: SetThinkingRequestParams

    class SetThinkingRequestParams(BaseModel):
        thinking: dict[str, Any]


SetThinkingRequestParams = SetThinkingRequest.SetThinkingRequestParams


class RewindFilesRequest(BaseModel):
    method: str = "control/rewindFiles"
    params: RewindFilesRequestParams

    class RewindFilesRequestParams(BaseModel):
        turn_id: str


RewindFilesRequestParams = RewindFilesRequest.RewindFilesRequestParams


class HookCallbackResponseRequest(BaseModel):
    method: str = "hook/callbackResponse"
    params: HookCallbackResponseRequestParams

    class HookCallbackResponseRequestParams(BaseModel):
        request_id: str
        output: Any = None
        error: str | None = None


HookCallbackResponseRequestParams = (
    HookCallbackResponseRequest.HookCallbackResponseRequestParams
)


class SessionResumeRequest(BaseModel):
    method: str = "session/resume"
    params: SessionResumeRequestParams

    class SessionResumeRequestParams(BaseModel):
        session_id: str
        prompt: str | None = None


SessionResumeRequestParams = SessionResumeRequest.SessionResumeRequestParams


# ---------------------------------------------------------------------------
# New notification param types (Phase 2)
# ---------------------------------------------------------------------------


class CommandInfo(BaseModel):
    name: str
    description: str | None = None


class TaskStartedParams(BaseModel):
    task_id: str
    task_type: str


class TaskCompletedParams(BaseModel):
    task_id: str
    result: str
    is_error: bool = False


class TurnInterruptedNotifParams(BaseModel):
    turn_id: str | None = None


class MaxTurnsReachedParams(BaseModel):
    max_turns: int | None = None


class ModelFallbackStartedParams(BaseModel):
    from_model: str
    to_model: str
    reason: str


class PermissionModeChangedParams(BaseModel):
    mode: str


class KeepAliveNotifParams(BaseModel):
    timestamp: int


class SessionEndedReason(str, Enum):
    completed = "completed"
    max_turns = "max_turns"
    max_budget = "max_budget"
    error = "error"
    user_interrupt = "user_interrupt"
    stdin_closed = "stdin_closed"


class SessionEndedParams(BaseModel):
    reason: SessionEndedReason


class SessionResultParams(BaseModel):
    session_id: str
    total_turns: int
    total_cost_cents: int | None = None
    duration_ms: int
    duration_api_ms: int | None = None
    usage: Usage
    stop_reason: SessionEndedReason
    structured_output: Any | None = None


class PromptSuggestionParams(BaseModel):
    suggestions: list[str]


# ---------------------------------------------------------------------------
# Server requests (server → client, require response)
# ---------------------------------------------------------------------------


class ServerRequest(BaseModel):
    """A request from the server that requires a client response."""

    method: str
    params: dict[str, Any] = {}

    def as_ask_for_approval(self) -> AskForApprovalParams | None:
        if self.method == "approval/askForApproval":
            return AskForApprovalParams.model_validate(self.params)
        return None

    def as_request_user_input(self) -> RequestUserInputParams | None:
        if self.method == "input/requestUserInput":
            return RequestUserInputParams.model_validate(self.params)
        return None

    def as_hook_callback(self) -> HookCallbackParams | None:
        if self.method == "hook/callback":
            return HookCallbackParams.model_validate(self.params)
        return None

    def as_mcp_route_message(self) -> McpRouteMessageParams | None:
        if self.method == "mcp/routeMessage":
            return McpRouteMessageParams.model_validate(self.params)
        return None


class PermissionSuggestion(BaseModel):
    behavior: str
    reason: str | None = None


class AskForApprovalParams(BaseModel):
    request_id: str
    tool_name: str
    input: Any = None
    description: str | None = None
    permission_suggestions: list[PermissionSuggestion] | None = None
    blocked_path: str | None = None
    decision_reason: str | None = None


class RequestUserInputParams(BaseModel):
    request_id: str
    message: str
    questions: Any | None = None


class HookCallbackParams(BaseModel):
    request_id: str
    callback_id: str
    event_type: str
    input: Any = None


class McpRouteMessageParams(BaseModel):
    request_id: str
    server_name: str
    message: Any


# ---------------------------------------------------------------------------
# Hook input/output types (typed payloads for hook callbacks)
# ---------------------------------------------------------------------------


class HookBehavior(str, Enum):
    allow = "allow"
    deny = "deny"
    error = "error"


class PreToolUseHookInput(BaseModel):
    tool_name: str
    tool_input: Any = None
    tool_use_id: str | None = None


class PostToolUseHookInput(BaseModel):
    tool_name: str
    tool_input: Any = None
    tool_output: str | None = None
    is_error: bool = False
    tool_use_id: str | None = None


class HookCallbackOutput(BaseModel):
    behavior: HookBehavior
    message: str | None = None
    updated_input: Any | None = None


class StopHookInput(BaseModel):
    stop_reason: str


class SubagentStartHookInput(BaseModel):
    agent_type: str
    prompt: str
    agent_id: str | None = None


class SubagentStopHookInput(BaseModel):
    agent_type: str
    agent_id: str
    output: str | None = None


class UserPromptSubmitHookInput(BaseModel):
    prompt: str


class NotificationHookInput(BaseModel):
    notification_type: str
    payload: Any = None


ApprovalResolveRequestParams = ApprovalResolveRequest.ApprovalResolveRequestParams
