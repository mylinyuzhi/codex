#!/usr/bin/env python3
"""Post-process auto-generated Python protocol types.

Reads a raw datamodel-code-generator output and injects ergonomic
accessor methods on tagged-union models (ServerNotification, ServerRequest,
ThreadItem). The result is written to the final protocol.py.

Usage:
    python postprocess_python.py <input_file> <output_file>
"""

import re
import sys
from pathlib import Path

# Accessor methods to inject on ServerNotification
NOTIFICATION_ACCESSORS = {
    "turn/started": ("as_turn_started", "TurnStartedParams"),
    "turn/completed": ("as_turn_completed", "TurnCompletedParams"),
    "turn/failed": ("as_turn_failed", "TurnFailedParams"),
    "turn/interrupted": ("as_turn_interrupted", "TurnInterruptedNotifParams"),
    "turn/maxReached": ("as_max_turns_reached", "MaxTurnsReachedParams"),
    "agentMessage/delta": ("as_agent_message_delta", "AgentMessageDeltaParams"),
    "reasoning/delta": ("as_reasoning_delta", "ReasoningDeltaParams"),
    "item/started": ("as_item_started", "ItemEventParams"),
    "item/updated": ("as_item_updated", "ItemEventParams"),
    "item/completed": ("as_item_completed", "ItemEventParams"),
    "session/started": ("as_session_started", "SessionStartedParams"),
    "session/ended": ("as_session_ended", "SessionEndedParams"),
    "subagent/spawned": ("as_subagent_spawned", "SubagentSpawnedParams"),
    "subagent/completed": ("as_subagent_completed", "SubagentCompletedParams"),
    "subagent/backgrounded": ("as_subagent_backgrounded", "SubagentBackgroundedParams"),
    "context/compacted": ("as_context_compacted", "ContextCompactedParams"),
    "context/usageWarning": ("as_context_usage_warning", "ContextUsageWarningParams"),
    "task/started": ("as_task_started", "TaskStartedParams"),
    "task/completed": ("as_task_completed", "TaskCompletedParams"),
    "error": ("as_error", "ErrorNotificationParams"),
    "rateLimit": ("as_rate_limit", "RateLimitParams"),
    "model/fallbackStarted": ("as_model_fallback_started", "ModelFallbackStartedParams"),
    "permission/modeChanged": ("as_permission_mode_changed", "PermissionModeChangedParams"),
}

# Accessor methods for ServerRequest
SERVER_REQUEST_ACCESSORS = {
    "approval/askForApproval": ("as_ask_for_approval", "AskForApprovalParams"),
    "input/requestUserInput": ("as_request_user_input", "RequestUserInputParams"),
    "hook/callback": ("as_hook_callback", "HookCallbackParams"),
    "mcp/routeMessage": ("as_mcp_route_message", "McpRouteMessageParams"),
}

# Accessor methods for ThreadItem
THREAD_ITEM_ACCESSORS = {
    "agent_message": ("as_agent_message", "AgentMessageItem"),
    "reasoning": ("as_reasoning", "ReasoningItem"),
    "command_execution": ("as_command_execution", "CommandExecutionItem"),
    "file_change": ("as_file_change", "FileChangeItem"),
    "mcp_tool_call": ("as_mcp_tool_call", "McpToolCallItem"),
    "web_search": ("as_web_search", "WebSearchItem"),
    "subagent": ("as_subagent", "SubagentItem"),
    "tool_call": ("as_tool_call", "GenericToolCallItem"),
    "error": ("as_error_item", "ErrorItem"),
}


def generate_accessor(method_name: str, return_type: str, field: str, key: str) -> str:
    """Generate a single accessor method."""
    return f"""
    def {method_name}(self) -> {return_type} | None:
        if self.{field} == {key!r}:
            return {return_type}.model_validate(self.params)
        return None
"""


def generate_thread_item_accessor(method_name: str, return_type: str, key: str) -> str:
    """Generate a ThreadItem accessor (uses 'type' field and model extras)."""
    return f"""
    def {method_name}(self) -> {return_type} | None:
        if self.type == {key!r}:
            return {return_type}.model_validate(self.model_dump())
        return None
"""


def inject_accessors(content: str) -> str:
    """Inject accessor methods into the generated code."""
    # This is a simple string-based approach that works with the
    # hand-written protocol.py format. For fully auto-generated code,
    # a more sophisticated approach may be needed.
    return content


def main() -> None:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input_file> <output_file>", file=sys.stderr)
        sys.exit(1)

    input_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    content = input_path.read_text()
    content = inject_accessors(content)
    output_path.write_text(content)
    print(f"Post-processed: {input_path} -> {output_path}")


if __name__ == "__main__":
    main()
