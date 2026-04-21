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

class ApprovalDecision(str, Enum):
    allow = 'allow'
    deny = 'deny'

class ExpandedView(str, Enum):
    none = 'none'
    tasks = 'tasks'
    teammates = 'teammates'

class FastModeState(str, Enum):
    off = 'off'
    cooldown = 'cooldown'
    on = 'on'

class FileChangeKind(str, Enum):
    create = 'create'
    modify = 'modify'
    delete = 'delete'

class HookOutcomeStatus(str, Enum):
    success = 'success'
    error = 'error'
    cancelled = 'cancelled'

class ItemStatus(str, Enum):
    in_progress = 'in_progress'
    completed = 'completed'
    failed = 'failed'
    declined = 'declined'

class McpConnectionStatus(str, Enum):
    connected = 'connected'
    disconnected = 'disconnected'

class PermissionBehavior(str, Enum):
    allow = 'allow'
    deny = 'deny'
    ask = 'ask'

class PermissionMode(str, Enum):
    default = 'default'
    auto = 'auto'
    bubble = 'bubble'

class PermissionRuleSource(str, Enum):
    user_settings = 'user_settings'
    project_settings = 'project_settings'
    local_settings = 'local_settings'
    flag_settings = 'flag_settings'
    policy_settings = 'policy_settings'
    cli_arg = 'cli_arg'
    command = 'command'
    session = 'session'

class PermissionUpdateDestination(str, Enum):
    user_settings = 'user_settings'
    project_settings = 'project_settings'
    local_settings = 'local_settings'
    session = 'session'
    cli_arg = 'cli_arg'

class RateLimitStatus(str, Enum):
    allowed = 'allowed'
    allowed_warning = 'allowed_warning'
    rejected = 'rejected'

class ReasoningEffort(str, Enum):
    none = 'none'
    minimal = 'minimal'
    low = 'low'
    medium = 'medium'
    high = 'high'
    x_high = 'x_high'

class SessionState(str, Enum):
    idle = 'idle'
    running = 'running'
    requires_action = 'requires_action'

class TaskCompletionStatus(str, Enum):
    completed = 'completed'
    failed = 'failed'
    stopped = 'stopped'

class TaskListStatus(str, Enum):
    pending = 'pending'
    in_progress = 'in_progress'
    completed = 'completed'


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

class AgentsKilledParams(BaseModel):
    count: int
    agent_ids: list[str] = []

class CompactionFailedParams(BaseModel):
    error: str
    attempts: int = 0

class ContentDeltaParams(BaseModel):
    delta: str
    item_id: str | None = None
    turn_id: str | None = None

class ContextClearedParams(BaseModel):
    new_mode: str | None = None

class ContextCompactedParams(BaseModel):
    removed_messages: int
    summary_tokens: int

class ContextUsageWarningParams(BaseModel):
    estimated_tokens: int
    percent_left: float
    warning_threshold: int

class CostWarningParams(BaseModel):
    current_cost_cents: int
    threshold_cents: int
    budget_cents: int | None = None

class ElicitationCompleteParams(BaseModel):
    elicitation_id: str
    mcp_server_name: str

class ErrorParams(BaseModel):
    message: str
    category: str | None = None
    retryable: bool = False

class FilesPersistedParams(BaseModel):
    files: list[PersistedFileInfo]
    processed_at: str
    failed: list[PersistedFileError] = []

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

class HookStartedParams(BaseModel):
    hook_event: str
    hook_id: str
    hook_name: str

class IdeDiagnosticsUpdatedParams(BaseModel):
    file_path: str
    new_count: int
    diagnostics: list[Any] = []

class IdeSelectionChangedParams(BaseModel):
    end_line: int
    file_path: str
    selected_text: str
    start_line: int

class LocalCommandOutputParams(BaseModel):
    content: Any

class McpStartupCompleteParams(BaseModel):
    servers: list[str]
    failed: list[str] = []

class McpStartupStatusParams(BaseModel):
    server: str
    status: McpConnectionStatus

class ModelFallbackParams(BaseModel):
    from_model: str
    reason: str
    to_model: str

class PermissionModeChangedParams(BaseModel):
    mode: PermissionMode
    bypass_available: bool = False

class PlanApprovalRequestedParams(BaseModel):
    from_: str = Field(alias='from')
    plan_content: str
    request_id: str
    plan_file_path: str | None = None

class PlanModeChangedParams(BaseModel):
    entered: bool
    approved: bool | None = None
    plan_file: str | None = None

class RateLimitParams(BaseModel):
    limit: int | None = None
    provider: str | None = None
    rate_limit_type: str | None = None
    remaining: int | None = None
    reset_at: int | None = None
    status: RateLimitStatus | None = None
    utilization: float | None = None

class RewindCompletedParams(BaseModel):
    messages_removed: int
    restored_files: int
    rewound_turn: int

class SandboxStateChangedParams(BaseModel):
    active: bool
    enforcement: str

class SessionEndedParams(BaseModel):
    reason: str

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

class SubagentBackgroundedParams(BaseModel):
    agent_id: str
    output_file: str

class SubagentCompletedParams(BaseModel):
    agent_id: str
    result: str
    is_error: bool = False

class SubagentProgressParams(BaseModel):
    agent_id: str
    current_step: int | None = None
    message: str | None = None
    summary: str | None = None
    total_steps: int | None = None

class SubagentSpawnedParams(BaseModel):
    agent_id: str
    agent_type: str
    description: str
    color: str | None = None

class SummarizeCompletedParams(BaseModel):
    from_turn: int
    summary_tokens: int

class TaskCompletedParams(BaseModel):
    output_file: str
    status: TaskCompletionStatus
    summary: str
    task_id: str
    tool_use_id: str | None = None
    usage: TaskUsage | None = None

class TaskPanelChangedParams(BaseModel):
    expanded_view: ExpandedView
    plan_tasks: list[TaskRecord]
    verification_nudge_pending: bool
    todos_by_agent: dict[str, list[TodoRecord]] = {}

class TaskProgressParams(BaseModel):
    description: str
    task_id: str
    usage: TaskUsage
    last_tool_name: str | None = None
    summary: str | None = None
    tool_use_id: str | None = None
    workflow_progress: list[Any] | None = None

class TaskStartedParams(BaseModel):
    description: str
    task_id: str
    prompt: str | None = None
    task_type: str | None = None
    tool_use_id: str | None = None
    workflow_name: str | None = None

class ToolProgressParams(BaseModel):
    elapsed_time_seconds: float
    tool_name: str
    tool_use_id: str
    parent_tool_use_id: str | None = None
    task_id: str | None = None

class ToolUseSummaryParams(BaseModel):
    preceding_tool_use_ids: list[str]
    summary: str

class TurnCompletedParams(BaseModel):
    usage: TokenUsage
    turn_id: str | None = None

class TurnFailedParams(BaseModel):
    error: str

class TurnInterruptedNotifParams(BaseModel):
    turn_id: str | None = None

class TurnStartedParams(BaseModel):
    turn_number: int
    turn_id: str | None = None

class WorktreeEnteredParams(BaseModel):
    branch: str
    worktree_path: str

class WorktreeExitedParams(BaseModel):
    action: str
    worktree_path: str

class ItemStartedParams(BaseModel):
    item: ThreadItem

class ItemUpdatedParams(BaseModel):
    item: ThreadItem

class ItemCompletedParams(BaseModel):
    item: ThreadItem

class ContextCompactionStartedParams(BaseModel):
    """Empty params for the wire-method `context/compactionStarted`."""

    model_config = {"extra": "allow"}

class ModelFallbackCompletedParams(BaseModel):
    """Empty params for the wire-method `model/fallbackCompleted`."""

    model_config = {"extra": "allow"}

class ModelFastModeChangedParams(BaseModel):
    active: bool

class PromptSuggestionParams(BaseModel):
    suggestions: list[str]

class KeepAliveNotifParams(BaseModel):
    timestamp: int

class QueueStateChangedParams(BaseModel):
    queued: int

class QueueCommandQueuedParams(BaseModel):
    id: str
    preview: str

class QueueCommandDequeuedParams(BaseModel):
    id: str

class RewindFailedParams(BaseModel):
    error: str

class SandboxViolationsDetectedParams(BaseModel):
    count: int

class AgentsRegisteredParams(BaseModel):
    agents: list[AgentInfo]

class SummarizeFailedParams(BaseModel):
    error: str

class StreamStallDetectedParams(BaseModel):
    turn_id: str | None = None

class StreamWatchdogWarningParams(BaseModel):
    elapsed_secs: float

class StreamRequestEndParams(BaseModel):
    usage: TokenUsage

class SessionStateChangedParams(BaseModel):
    state: SessionState

class TurnMaxReachedParams(BaseModel):
    max_turns: int | None = None


# ---------------------------------------------------------------------------
# Notification wire-method constants
# ---------------------------------------------------------------------------

class NotificationMethod(str, Enum):
    """Wire-method identifier for every `ServerNotification` variant. Mirrors the Rust `NotificationMethod` enum. Members inherit from `str`, so equality with raw wire strings Just Works."""

    SESSION_STARTED = 'session/started'
    SESSION_RESULT = 'session/result'
    SESSION_ENDED = 'session/ended'
    TURN_STARTED = 'turn/started'
    TURN_COMPLETED = 'turn/completed'
    TURN_FAILED = 'turn/failed'
    TURN_INTERRUPTED = 'turn/interrupted'
    ITEM_STARTED = 'item/started'
    ITEM_UPDATED = 'item/updated'
    ITEM_COMPLETED = 'item/completed'
    AGENT_MESSAGE_DELTA = 'agentMessage/delta'
    REASONING_DELTA = 'reasoning/delta'
    SUBAGENT_SPAWNED = 'subagent/spawned'
    SUBAGENT_COMPLETED = 'subagent/completed'
    SUBAGENT_BACKGROUNDED = 'subagent/backgrounded'
    SUBAGENT_PROGRESS = 'subagent/progress'
    MCP_STARTUP_STATUS = 'mcp/startupStatus'
    MCP_STARTUP_COMPLETE = 'mcp/startupComplete'
    CONTEXT_COMPACTED = 'context/compacted'
    CONTEXT_USAGE_WARNING = 'context/usageWarning'
    CONTEXT_COMPACTION_STARTED = 'context/compactionStarted'
    CONTEXT_COMPACTION_FAILED = 'context/compactionFailed'
    CONTEXT_CLEARED = 'context/cleared'
    TASK_STARTED = 'task/started'
    TASK_COMPLETED = 'task/completed'
    TASK_PROGRESS = 'task/progress'
    TASK_PANEL_CHANGED = 'task_panel/changed'
    PLAN_APPROVAL_REQUESTED = 'plan_approval/requested'
    AGENTS_KILLED = 'agents/killed'
    MODEL_FALLBACK_STARTED = 'model/fallbackStarted'
    MODEL_FALLBACK_COMPLETED = 'model/fallbackCompleted'
    MODEL_FAST_MODE_CHANGED = 'model/fastModeChanged'
    PERMISSION_MODE_CHANGED = 'permission/modeChanged'
    PROMPT_SUGGESTION = 'prompt/suggestion'
    ERROR = 'error'
    RATE_LIMIT = 'rateLimit'
    KEEP_ALIVE = 'keepAlive'
    IDE_SELECTION_CHANGED = 'ide/selectionChanged'
    IDE_DIAGNOSTICS_UPDATED = 'ide/diagnosticsUpdated'
    PLAN_MODE_CHANGED = 'plan/modeChanged'
    QUEUE_STATE_CHANGED = 'queue/stateChanged'
    QUEUE_COMMAND_QUEUED = 'queue/commandQueued'
    QUEUE_COMMAND_DEQUEUED = 'queue/commandDequeued'
    REWIND_COMPLETED = 'rewind/completed'
    REWIND_FAILED = 'rewind/failed'
    COST_WARNING = 'cost/warning'
    SANDBOX_STATE_CHANGED = 'sandbox/stateChanged'
    SANDBOX_VIOLATIONS_DETECTED = 'sandbox/violationsDetected'
    AGENTS_REGISTERED = 'agents/registered'
    HOOK_STARTED = 'hook/started'
    HOOK_PROGRESS = 'hook/progress'
    HOOK_RESPONSE = 'hook/response'
    WORKTREE_ENTERED = 'worktree/entered'
    WORKTREE_EXITED = 'worktree/exited'
    SUMMARIZE_COMPLETED = 'summarize/completed'
    SUMMARIZE_FAILED = 'summarize/failed'
    STREAM_STALL_DETECTED = 'stream/stallDetected'
    STREAM_WATCHDOG_WARNING = 'stream/watchdogWarning'
    STREAM_REQUEST_END = 'stream/requestEnd'
    SESSION_STATE_CHANGED = 'session/stateChanged'
    TURN_MAX_REACHED = 'turn/maxReached'
    LOCAL_COMMAND_OUTPUT = 'localCommand/output'
    FILES_PERSISTED = 'files/persisted'
    ELICITATION_COMPLETE = 'elicitation/complete'
    TOOL_USE_SUMMARY = 'tool/useSummary'
    TOOL_PROGRESS = 'tool/progress'


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

    def as_item_started(self) -> ItemStartedParams | None:
        if self.method == 'item/started':
            return ItemStartedParams.model_validate(self.params)
        return None

    def as_item_updated(self) -> ItemUpdatedParams | None:
        if self.method == 'item/updated':
            return ItemUpdatedParams.model_validate(self.params)
        return None

    def as_item_completed(self) -> ItemCompletedParams | None:
        if self.method == 'item/completed':
            return ItemCompletedParams.model_validate(self.params)
        return None

    def as_agent_message_delta(self) -> ContentDeltaParams | None:
        if self.method == 'agentMessage/delta':
            return ContentDeltaParams.model_validate(self.params)
        return None

    def as_reasoning_delta(self) -> ContentDeltaParams | None:
        if self.method == 'reasoning/delta':
            return ContentDeltaParams.model_validate(self.params)
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

    def as_context_compaction_started(self) -> ContextCompactionStartedParams | None:
        if self.method == 'context/compactionStarted':
            return ContextCompactionStartedParams.model_validate(self.params)
        return None

    def as_context_compaction_failed(self) -> CompactionFailedParams | None:
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

    def as_task_panel_changed(self) -> TaskPanelChangedParams | None:
        if self.method == 'task_panel/changed':
            return TaskPanelChangedParams.model_validate(self.params)
        return None

    def as_plan_approval_requested(self) -> PlanApprovalRequestedParams | None:
        if self.method == 'plan_approval/requested':
            return PlanApprovalRequestedParams.model_validate(self.params)
        return None

    def as_agents_killed(self) -> AgentsKilledParams | None:
        if self.method == 'agents/killed':
            return AgentsKilledParams.model_validate(self.params)
        return None

    def as_model_fallback_started(self) -> ModelFallbackParams | None:
        if self.method == 'model/fallbackStarted':
            return ModelFallbackParams.model_validate(self.params)
        return None

    def as_model_fallback_completed(self) -> ModelFallbackCompletedParams | None:
        if self.method == 'model/fallbackCompleted':
            return ModelFallbackCompletedParams.model_validate(self.params)
        return None

    def as_model_fast_mode_changed(self) -> ModelFastModeChangedParams | None:
        if self.method == 'model/fastModeChanged':
            return ModelFastModeChangedParams.model_validate(self.params)
        return None

    def as_permission_mode_changed(self) -> PermissionModeChangedParams | None:
        if self.method == 'permission/modeChanged':
            return PermissionModeChangedParams.model_validate(self.params)
        return None

    def as_prompt_suggestion(self) -> PromptSuggestionParams | None:
        if self.method == 'prompt/suggestion':
            return PromptSuggestionParams.model_validate(self.params)
        return None

    def as_error(self) -> ErrorParams | None:
        if self.method == 'error':
            return ErrorParams.model_validate(self.params)
        return None

    def as_rate_limit(self) -> RateLimitParams | None:
        if self.method == 'rateLimit':
            return RateLimitParams.model_validate(self.params)
        return None

    def as_keep_alive(self) -> KeepAliveParams | None:
        if self.method == 'keepAlive':
            return KeepAliveParams.model_validate(self.params)
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

    def as_queue_command_queued(self) -> QueueCommandQueuedParams | None:
        if self.method == 'queue/commandQueued':
            return QueueCommandQueuedParams.model_validate(self.params)
        return None

    def as_queue_command_dequeued(self) -> QueueCommandDequeuedParams | None:
        if self.method == 'queue/commandDequeued':
            return QueueCommandDequeuedParams.model_validate(self.params)
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

    def as_session_state_changed(self) -> SessionStateChangedParams | None:
        if self.method == 'session/stateChanged':
            return SessionStateChangedParams.model_validate(self.params)
        return None

    def as_turn_max_reached(self) -> TurnMaxReachedParams | None:
        if self.method == 'turn/maxReached':
            return TurnMaxReachedParams.model_validate(self.params)
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

class HookCallbackParams(BaseModel):
    callback_id: str
    input: Any
    request_id: str
    tool_use_id: str | None = None

class McpRouteMessageParams(BaseModel):
    message: Any
    request_id: str
    server_name: str

class RequestUserInputParams(BaseModel):
    prompt: str
    request_id: str
    choices: list[str] | None = None
    default: str | None = None
    description: str | None = None

class ServerCancelRequestParams(BaseModel):
    request_id: str
    reason: str | None = None


class ServerRequestMethod(str, Enum):
    """Wire-method identifier for every `ServerRequest` variant. Mirrors the Rust `ServerRequestMethod` enum."""

    APPROVAL_ASK_FOR_APPROVAL = 'approval/askForApproval'
    INPUT_REQUEST_USER_INPUT = 'input/requestUserInput'
    MCP_ROUTE_MESSAGE = 'mcp/routeMessage'
    HOOK_CALLBACK = 'hook/callback'
    CONTROL_CANCEL_REQUEST = 'control/cancelRequest'


class ServerRequest(BaseModel):
    """A request from the server that requires a client response."""

    method: str
    params: dict[str, Any] = {}

    def as_approval_ask_for_approval(self) -> AskForApprovalParams | None:
        if self.method == 'approval/askForApproval':
            return AskForApprovalParams.model_validate(self.params)
        return None

    def as_input_request_user_input(self) -> RequestUserInputParams | None:
        if self.method == 'input/requestUserInput':
            return RequestUserInputParams.model_validate(self.params)
        return None

    def as_mcp_route_message(self) -> McpRouteMessageParams | None:
        if self.method == 'mcp/routeMessage':
            return McpRouteMessageParams.model_validate(self.params)
        return None

    def as_hook_callback(self) -> HookCallbackParams | None:
        if self.method == 'hook/callback':
            return HookCallbackParams.model_validate(self.params)
        return None

    def as_control_cancel_request(self) -> ServerCancelRequestParams | None:
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

# Union type: see Rust source for variants
PermissionUpdate = Any


# ---------------------------------------------------------------------------
# Hook input/output types
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Client request params
# ---------------------------------------------------------------------------

class ApprovalResolveParams(BaseModel):
    decision: ApprovalDecision
    request_id: str
    feedback: str | None = None
    permission_update: PermissionUpdate | None = None
    updated_input: Any = None

class CancelRequestParams(BaseModel):
    request_id: str
    reason: str | None = None

class ConfigApplyFlagsParams(BaseModel):
    settings: dict[str, Any]

class ConfigWriteParams(BaseModel):
    key: str
    value: Any
    scope: str | None = None

class ElicitationResolveParams(BaseModel):
    approved: bool
    mcp_server_name: str
    request_id: str
    values: dict[str, Any] = {}

class HookCallbackResponseParams(BaseModel):
    callback_id: str
    output: Any

class InitializeParams(BaseModel):
    agent_progress_summaries: bool | None = None
    agents: dict[str, Any] | None = None
    append_system_prompt: str | None = None
    hooks: dict[str, list[HookCallbackMatcher]] | None = None
    json_schema: Any = None
    prompt_suggestions: bool | None = None
    sdk_mcp_servers: list[str] | None = None
    system_prompt: str | None = None

class McpReconnectParams(BaseModel):
    server_name: str

class McpRouteMessageResponseParams(BaseModel):
    message: Any
    request_id: str

class McpSetServersParams(BaseModel):
    servers: dict[str, Any]

class McpToggleParams(BaseModel):
    enabled: bool
    server_name: str

class RewindFilesParams(BaseModel):
    user_message_id: str
    dry_run: bool = False

class SessionArchiveParams(BaseModel):
    session_id: str

class SessionReadParams(BaseModel):
    session_id: str
    cursor: str | None = None
    limit: int | None = None

class SessionResumeParams(BaseModel):
    session_id: str

class SessionStartParams(BaseModel):
    append_system_prompt: str | None = None
    cwd: str | None = None
    initial_prompt: str | None = None
    max_budget_usd: float | None = None
    max_turns: int | None = None
    model: str | None = None
    permission_mode: PermissionMode | None = None
    system_prompt: str | None = None

class SetModelParams(BaseModel):
    model: str | None = None

class SetPermissionModeParams(BaseModel):
    mode: PermissionMode

class SetThinkingParams(BaseModel):
    thinking_level: ThinkingLevel | None = None

class StopTaskParams(BaseModel):
    task_id: str

class TurnStartParams(BaseModel):
    prompt: str
    permission_mode: PermissionMode | None = None
    thinking_level: ThinkingLevel | None = None

class UpdateEnvParams(BaseModel):
    env: dict[str, str]

class UserInputResolveParams(BaseModel):
    answer: str
    request_id: str


# ---------------------------------------------------------------------------
# Client request wire-method constants
# ---------------------------------------------------------------------------

class ClientRequestMethod(str, Enum):
    """Wire-method identifier for every `ClientRequest` variant. Mirrors the Rust `ClientRequestMethod` enum."""

    INITIALIZE = 'initialize'
    SESSION_START = 'session/start'
    SESSION_RESUME = 'session/resume'
    SESSION_LIST = 'session/list'
    SESSION_READ = 'session/read'
    SESSION_ARCHIVE = 'session/archive'
    TURN_START = 'turn/start'
    TURN_INTERRUPT = 'turn/interrupt'
    APPROVAL_RESOLVE = 'approval/resolve'
    INPUT_RESOLVE_USER_INPUT = 'input/resolveUserInput'
    ELICITATION_RESOLVE = 'elicitation/resolve'
    CONTROL_SET_MODEL = 'control/setModel'
    CONTROL_SET_PERMISSION_MODE = 'control/setPermissionMode'
    CONTROL_SET_THINKING = 'control/setThinking'
    CONTROL_STOP_TASK = 'control/stopTask'
    CONTROL_REWIND_FILES = 'control/rewindFiles'
    CONTROL_UPDATE_ENV = 'control/updateEnv'
    CONTROL_KEEP_ALIVE = 'control/keepAlive'
    CONTROL_CANCEL_REQUEST = 'control/cancelRequest'
    CONFIG_READ = 'config/read'
    CONFIG_VALUE_WRITE = 'config/value/write'
    HOOK_CALLBACK_RESPONSE = 'hook/callbackResponse'
    MCP_ROUTE_MESSAGE_RESPONSE = 'mcp/routeMessageResponse'
    MCP_STATUS = 'mcp/status'
    CONTEXT_USAGE = 'context/usage'
    MCP_SET_SERVERS = 'mcp/setServers'
    MCP_RECONNECT = 'mcp/reconnect'
    MCP_TOGGLE = 'mcp/toggle'
    PLUGIN_RELOAD = 'plugin/reload'
    CONFIG_APPLY_FLAGS = 'config/applyFlags'


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

class SessionListRequest(BaseModel):
    method: str = 'session/list'
    params: SessionListRequestParams

    class SessionListRequestParams(BaseModel):
        model_config = {"extra": "allow"}

SessionListRequestParams = SessionListRequest.SessionListRequestParams

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

class TurnInterruptRequest(BaseModel):
    method: str = 'turn/interrupt'
    params: TurnInterruptRequestParams

    class TurnInterruptRequestParams(BaseModel):
        model_config = {"extra": "allow"}

TurnInterruptRequestParams = TurnInterruptRequest.TurnInterruptRequestParams

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

class ElicitationResolveRequest(BaseModel):
    method: str = 'elicitation/resolve'
    params: ElicitationResolveRequestParams

    class ElicitationResolveRequestParams(ElicitationResolveParams):
        pass

ElicitationResolveRequestParams = ElicitationResolveRequest.ElicitationResolveRequestParams

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

class KeepAliveRequest(BaseModel):
    method: str = 'control/keepAlive'
    params: KeepAliveRequestParams

    class KeepAliveRequestParams(BaseModel):
        model_config = {"extra": "allow"}

KeepAliveRequestParams = KeepAliveRequest.KeepAliveRequestParams

class CancelRequest(BaseModel):
    method: str = 'control/cancelRequest'
    params: CancelRequestParams

    class CancelRequestParams(CancelRequestParams):
        pass

CancelRequestParams = CancelRequest.CancelRequestParams

class ConfigReadRequest(BaseModel):
    method: str = 'config/read'
    params: ConfigReadRequestParams

    class ConfigReadRequestParams(BaseModel):
        model_config = {"extra": "allow"}

ConfigReadRequestParams = ConfigReadRequest.ConfigReadRequestParams

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

class McpStatusRequest(BaseModel):
    method: str = 'mcp/status'
    params: McpStatusRequestParams

    class McpStatusRequestParams(BaseModel):
        model_config = {"extra": "allow"}

McpStatusRequestParams = McpStatusRequest.McpStatusRequestParams

class ContextUsageRequest(BaseModel):
    method: str = 'context/usage'
    params: ContextUsageRequestParams

    class ContextUsageRequestParams(BaseModel):
        model_config = {"extra": "allow"}

ContextUsageRequestParams = ContextUsageRequest.ContextUsageRequestParams

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

class PluginReloadRequest(BaseModel):
    method: str = 'plugin/reload'
    params: PluginReloadRequestParams

    class PluginReloadRequestParams(BaseModel):
        model_config = {"extra": "allow"}

PluginReloadRequestParams = PluginReloadRequest.PluginReloadRequestParams

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
    kind: FileChangeKind
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

class TaskRecord(BaseModel):
    id: str
    status: TaskListStatus
    subject: str
    activeForm: str | None = None
    blockedBy: list[str] = []
    blocks: list[str] = []
    description: str = ''
    metadata: dict[str, Any] | None = None
    owner: str | None = None

class TaskUsage(BaseModel):
    duration_ms: int
    tool_uses: int
    total_tokens: int

class ThinkingLevel(BaseModel):
    effort: ReasoningEffort
    budget_tokens: int | None = None
    options: dict[str, Any] | None = None

class TodoRecord(BaseModel):
    activeForm: str
    content: str
    status: str

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


class CommandExecutionItem(BaseModel):
    """Stub for CommandExecutionItem pending coco-rs schema emission."""
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


class HookExecutedParams(BaseModel):
    """Stub for HookExecutedParams pending coco-rs schema emission."""
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


class Usage(BaseModel):
    """Stub for Usage pending coco-rs schema emission."""
    model_config = {"extra": "allow"}

