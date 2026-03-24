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

from cocode_sdk.client import CocodeClient
from cocode_sdk.generated.protocol import (
    AgentMessageDeltaParams,
    AgentMessageItem,
    ApprovalDecision,
    CommandExecutionItem,
    ContextCompactedParams,
    ContextUsageWarningParams,
    ErrorItem,
    ErrorNotificationParams,
    FileChange,
    FileChangeItem,
    FileChangeKind,
    GenericToolCallItem,
    ItemEventParams,
    ItemStatus,
    McpServerFailure,
    McpServerInfoParams,
    McpStartupCompleteParams,
    McpStartupStatusParams,
    McpToolCallItem,
    RateLimitParams,
    ReasoningDeltaParams,
    ReasoningItem,
    ServerNotification,
    SessionStartedParams,
    SubagentBackgroundedParams,
    SubagentCompletedParams,
    SubagentItem,
    SubagentSpawnedParams,
    ThreadItem,
    TurnCompletedParams,
    TurnFailedParams,
    TurnStartedParams,
    Usage,
    WebSearchItem,
)
from cocode_sdk.query import query

__version__ = "0.1.0"

__all__ = [
    "__version__",
    # Functions
    "query",
    # Client
    "CocodeClient",
    # Notification types
    "ServerNotification",
    "AgentMessageDeltaParams",
    "ContextCompactedParams",
    "ContextUsageWarningParams",
    "ErrorNotificationParams",
    "ItemEventParams",
    "McpServerFailure",
    "McpServerInfoParams",
    "McpStartupCompleteParams",
    "McpStartupStatusParams",
    "RateLimitParams",
    "ReasoningDeltaParams",
    "SessionStartedParams",
    "SubagentBackgroundedParams",
    "SubagentCompletedParams",
    "SubagentSpawnedParams",
    "TurnCompletedParams",
    "TurnFailedParams",
    "TurnStartedParams",
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
    "ItemStatus",
    # Usage
    "Usage",
]
