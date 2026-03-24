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


# ---------------------------------------------------------------------------
# Client requests
# ---------------------------------------------------------------------------


class ApprovalDecision(str, Enum):
    approve = "approve"
    approve_session = "approve_session"
    deny = "deny"


class SessionStartRequest(BaseModel):
    method: str = "session/start"
    params: SessionStartRequestParams

    class SessionStartRequestParams(BaseModel):
        prompt: str
        model: str | None = None
        max_turns: int | None = None
        cwd: str | None = None
        system_prompt_suffix: str | None = None
        permission_mode: str | None = None
        env: dict[str, str] | None = None


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
