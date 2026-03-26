"""cocode SDK — programmatic access to the cocode multi-provider LLM CLI.

Two usage patterns:

1. One-shot query (simplest)::

    from cocode_sdk import query

    async for event in query("Fix the bug"):
        print(event.method, event.params)

2. Multi-turn client::

    from cocode_sdk import CocodeClient

    async with CocodeClient(prompt="Fix the bug") as client:
        async for event in client.events():
            print(event.method)
        async for event in client.send("Now add tests"):
            print(event.method)
"""

from cocode_sdk.client import CanUseTool, CocodeClient, HookHandler
from cocode_sdk.decorators import HookDefinition, hook
from cocode_sdk.structured import TypedClient
from cocode_sdk.tools import ToolDefinition, tool
from cocode_sdk.generated.protocol import (
    AgentDefinitionConfig,
    AgentHookConfig,
    AgentIsolationMode,
    AgentMemoryScope,
    AgentMessageDeltaParams,
    AgentMessageItem,
    ApprovalDecision,
    AskForApprovalParams,
    CommandExecutionItem,
    CommandInfo,
    ContextCompactedParams,
    ContextUsageWarningParams,
    ErrorItem,
    ErrorNotificationParams,
    FileChange,
    FileChangeItem,
    FileChangeKind,
    GenericToolCallItem,
    HookBehavior,
    HookCallbackConfig,
    HookCallbackOutput,
    HookCallbackParams,
    HttpMcpServerConfig,
    ItemEventParams,
    ItemStatus,
    KeepAliveNotifParams,
    MaxTurnsReachedParams,
    McpRouteMessageParams,
    McpServerFailure,
    McpServerInfoParams,
    McpStartupCompleteParams,
    McpStartupStatusParams,
    McpToolCallItem,
    ModelFallbackStartedParams,
    NotificationHookInput,
    PermissionModeChangedParams,
    PermissionSuggestion,
    PostToolUseHookInput,
    PreToolUseHookInput,
    PromptSuggestionParams,
    RateLimitParams,
    ReasoningDeltaParams,
    ReasoningItem,
    RequestUserInputParams,
    ServerNotification,
    ServerRequest,
    SessionEndedParams,
    SessionEndedReason,
    SessionResultParams,
    SessionStartedParams,
    SseMcpServerConfig,
    StdioMcpServerConfig,
    StopHookInput,
    SubagentBackgroundedParams,
    SubagentCompletedParams,
    SubagentItem,
    SubagentStartHookInput,
    SubagentStopHookInput,
    SubagentSpawnedParams,
    TaskCompletedParams,
    TaskStartedParams,
    ThreadItem,
    TurnCompletedParams,
    TurnFailedParams,
    TurnInterruptedNotifParams,
    TurnStartedParams,
    Usage,
    UserPromptSubmitHookInput,
    WebSearchItem,
)
from cocode_sdk.query import query

__version__ = "0.1.0"

__all__ = [
    "__version__",
    # Functions
    "query",
    # Client
    "CanUseTool",
    "CocodeClient",
    "HookHandler",
    "TypedClient",
    # Decorators
    "tool",
    "ToolDefinition",
    "hook",
    "HookDefinition",
    # Config types
    "AgentDefinitionConfig",
    "AgentHookConfig",
    "AgentIsolationMode",
    "AgentMemoryScope",
    "CommandInfo",
    "HookCallbackConfig",
    "HttpMcpServerConfig",
    "SseMcpServerConfig",
    "StdioMcpServerConfig",
    # Notification types
    "ServerNotification",
    "AgentMessageDeltaParams",
    "ContextCompactedParams",
    "ContextUsageWarningParams",
    "ErrorNotificationParams",
    "ItemEventParams",
    "KeepAliveNotifParams",
    "MaxTurnsReachedParams",
    "McpServerFailure",
    "McpServerInfoParams",
    "McpStartupCompleteParams",
    "McpStartupStatusParams",
    "ModelFallbackStartedParams",
    "PermissionModeChangedParams",
    "RateLimitParams",
    "ReasoningDeltaParams",
    "SessionEndedParams",
    "SessionStartedParams",
    "SubagentBackgroundedParams",
    "SubagentCompletedParams",
    "SubagentSpawnedParams",
    "TaskCompletedParams",
    "TaskStartedParams",
    "TurnCompletedParams",
    "TurnFailedParams",
    "TurnInterruptedNotifParams",
    "TurnStartedParams",
    # Server request types
    "ServerRequest",
    "AskForApprovalParams",
    "HookCallbackParams",
    "McpRouteMessageParams",
    "RequestUserInputParams",
    # Item types
    "ThreadItem",
    "AgentMessageItem",
    "CommandExecutionItem",
    "ErrorItem",
    "FileChange",
    "FileChangeItem",
    "FileChangeKind",
    "GenericToolCallItem",
    "McpToolCallItem",
    "ReasoningItem",
    "SubagentItem",
    "WebSearchItem",
    # Enums
    "ApprovalDecision",
    "HookBehavior",
    "ItemStatus",
    "SessionEndedReason",
    # Hook input/output types
    "HookCallbackOutput",
    "PreToolUseHookInput",
    "PostToolUseHookInput",
    "StopHookInput",
    "SubagentStartHookInput",
    "SubagentStopHookInput",
    "UserPromptSubmitHookInput",
    "NotificationHookInput",
    # New notification types
    "SessionResultParams",
    "PromptSuggestionParams",
    "PermissionSuggestion",
    # Usage
    "Usage",
]
