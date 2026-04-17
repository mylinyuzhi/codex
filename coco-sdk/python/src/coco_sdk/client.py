"""Multi-turn coco client for interactive sessions."""

from __future__ import annotations

import json
import logging
from typing import TYPE_CHECKING, Any, AsyncIterator, Callable, Awaitable

logger = logging.getLogger(__name__)

if TYPE_CHECKING:
    from coco_sdk.tools import ToolDefinition

from coco_sdk._internal.transport import Transport
from coco_sdk._internal.transport.subprocess_cli import SubprocessCLITransport
from coco_sdk.generated.protocol import (
    AgentDefinitionConfig,
    ApprovalDecision,
    ApprovalResolveRequest,
    CancelRequest,
    ConfigReadRequest,
    ConfigWriteRequest,
    HookCallbackConfig,
    HookCallbackResponseRequest,
    McpServerConfig,
    RewindFilesRequest,
    ServerNotification,
    ServerRequest,
    SessionArchiveRequest,
    SessionListRequest,
    SessionReadRequest,
    SessionResumeRequest,
    SessionStartRequest,
    SetModelRequest,
    SetPermissionModeRequest,
    SetThinkingRequest,
    StopTaskRequest,
    TurnCompletedParams,
    TurnInterruptRequest,
    TurnStartRequest,
    UpdateEnvRequest,
    UserInputResolveRequest,
)

def _safe_parse_notification(line_data: dict[str, Any]) -> ServerNotification:
    """Parse a notification dict, falling back to raw method+params on error."""
    try:
        return ServerNotification.model_validate(line_data)
    except Exception as exc:
        logger.warning("Failed to parse notification %s: %s", line_data.get("method"), exc)
        return ServerNotification(
            method=line_data.get("method", "unknown"),
            params=line_data.get("params", {}),
        )


# Callback type for permission decisions
CanUseTool = Callable[[str, dict[str, Any]], Awaitable[ApprovalDecision]]

# Hook handler: (callback_id, event_type, input) -> output
HookHandler = Callable[[str, str, dict[str, Any]], Awaitable[dict[str, Any]]]


class CocoClient:
    """Multi-turn client for coco sessions with bidirectional control.

    Example::

        async with CocoClient(prompt="Fix the bug in main.rs") as client:
            async for event in client.events():
                print(event.method, event.params)

            # Send follow-up
            async for event in client.send("Now add tests"):
                print(event.method, event.params)

    With approval callback::

        async def on_approval(tool_name: str, input: dict) -> ApprovalDecision:
            if tool_name in ("Read", "Glob", "Grep"):
                return ApprovalDecision.approve
            return ApprovalDecision.deny

        async with CocoClient(prompt="...", can_use_tool=on_approval) as client:
            async for event in client.events():
                print(event.method)
    """

    def __init__(
        self,
        prompt: str,
        *,
        model: str | None = None,
        max_turns: int | None = None,
        cwd: str | None = None,
        system_prompt_suffix: str | None = None,
        system_prompt: str | None = None,
        permission_mode: str | None = None,
        env: dict[str, str] | None = None,
        binary_path: str | None = None,
        transport: Transport | None = None,
        can_use_tool: CanUseTool | None = None,
        agents: dict[str, AgentDefinitionConfig] | None = None,
        hooks: list[HookCallbackConfig] | None = None,
        mcp_servers: dict[str, McpServerConfig] | None = None,
        tools: list[ToolDefinition] | None = None,
        sandbox: dict[str, Any] | None = None,
        thinking: dict[str, Any] | None = None,
        max_budget_cents: int | None = None,
        disable_builtin_agents: bool | None = None,
        output_format: Any | None = None,
    ):
        self._initial_prompt = prompt
        self._model = model
        self._max_turns = max_turns
        self._cwd = cwd
        self._system_prompt_suffix = system_prompt_suffix
        self._system_prompt = system_prompt
        self._permission_mode = permission_mode
        self._env = env
        self._agents = agents
        self._hooks = hooks
        self._mcp_servers = mcp_servers
        self._tools = tools
        self._sandbox = sandbox
        self._thinking = thinking
        self._max_budget_cents = max_budget_cents
        self._disable_builtin_agents = disable_builtin_agents
        self._output_format = output_format
        self._transport = transport or SubprocessCLITransport(
            binary_path=binary_path,
            cwd=cwd,
            env=env,
        )
        self._can_use_tool = can_use_tool
        self._hook_handlers: dict[str, HookHandler] = {}
        self._tool_registry: dict[str, ToolDefinition] = {}
        self._started = False

        # Build tool registry from @tool() decorated functions
        if tools:
            for tool_def in tools:
                self._tool_registry[tool_def.server_name] = tool_def

    async def __aenter__(self) -> CocoClient:
        await self.start()
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    async def start(self) -> None:
        """Start the session by sending session/start."""
        await self._transport.start()
        self._started = True

        # Build MCP servers dict, merging user-provided and @tool() generated
        mcp_servers = {}
        if self._mcp_servers:
            mcp_servers.update({
                k: v.model_dump(exclude_none=True)
                for k, v in self._mcp_servers.items()
            })
        if self._tools:
            for tool_def in self._tools:
                name, config = tool_def.to_sdk_mcp_config()
                mcp_servers[name] = config

        # Send session/start request
        request = SessionStartRequest(
            params=SessionStartRequest.SessionStartRequestParams(
                prompt=self._initial_prompt,
                model=self._model,
                max_turns=self._max_turns,
                cwd=self._cwd,
                system_prompt_suffix=self._system_prompt_suffix,
                system_prompt=self._system_prompt,
                permission_mode=self._permission_mode,
                env=self._env,
                agents={
                    k: v.model_dump(exclude_none=True)
                    for k, v in self._agents.items()
                } if self._agents else None,
                mcp_servers=mcp_servers or None,
                hooks=[h.model_dump(exclude_none=True) for h in self._hooks]
                if self._hooks else None,
                sandbox=self._sandbox,
                thinking=self._thinking,
                max_budget_cents=self._max_budget_cents,
                disable_builtin_agents=self._disable_builtin_agents,
                output_format=self._output_format,
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def events(self) -> AsyncIterator[ServerNotification]:
        """Yield events from the current turn.

        Automatically handles approval requests if a ``can_use_tool``
        callback was provided. Otherwise, ``ServerRequest`` messages
        (approval/askForApproval, input/requestUserInput) are yielded
        as-is for manual handling.
        """
        async for line_data in self._transport.read_lines():
            method = line_data.get("method", "")

            # Detect ServerRequest (approval/question) vs ServerNotification
            if method == "approval/askForApproval":
                if self._can_use_tool:
                    params = line_data.get("params", {})
                    decision = await self._can_use_tool(
                        params.get("tool_name", ""),
                        params.get("input", {}),
                    )
                    await self.approve(params.get("request_id", ""), decision)
                    continue
                # No callback — yield as notification for manual handling
                yield _safe_parse_notification(line_data)
                continue

            if method == "hook/callback":
                params = line_data.get("params", {})
                cb_id = params.get("callback_id", "")
                handler = self._hook_handlers.get(cb_id)
                if handler:
                    try:
                        output = await handler(
                            cb_id,
                            params.get("event_type", ""),
                            params.get("input", {}),
                        )
                    except Exception as exc:
                        await self.respond_to_hook(
                            params.get("request_id", ""), error=str(exc)
                        )
                    else:
                        if not isinstance(output, dict):
                            await self.respond_to_hook(
                                params.get("request_id", ""),
                                error=f"Hook handler must return dict, got {type(output).__name__}",
                            )
                        else:
                            await self.respond_to_hook(
                                params.get("request_id", ""), output=output
                            )
                    continue
                # No handler — yield for manual handling
                yield _safe_parse_notification(line_data)
                continue

            if method == "mcp/routeMessage":
                params = line_data.get("params", {})
                server_name = params.get("server_name", "")
                request_id = params.get("request_id", "")
                message = params.get("message", {})
                tool_def = self._tool_registry.get(server_name)
                if tool_def and message.get("method") == "tools/call":
                    mcp_params = message.get("params", {})
                    try:
                        result = await tool_def.invoke(mcp_params.get("arguments", {}))
                        result_str = result if isinstance(result, str) else json.dumps(result)
                        await self._respond_to_mcp_route(
                            request_id, {"result": result_str}
                        )
                    except Exception as exc:
                        await self._respond_to_mcp_route(
                            request_id, None, error=str(exc)
                        )
                    continue
                # No handler or unsupported method — yield as notification
                yield _safe_parse_notification(line_data)
                continue

            if method == "input/requestUserInput":
                # Always yield for manual handling (no auto-response)
                yield _safe_parse_notification(line_data)
                continue

            # Regular notification
            event = _safe_parse_notification(line_data)
            yield event

            # Stop after turn completion or failure
            if method in ("turn/completed", "turn/failed"):
                break

    async def send(self, text: str) -> AsyncIterator[ServerNotification]:
        """Send a follow-up message and yield events from the new turn."""
        request = TurnStartRequest(
            params=TurnStartRequest.TurnStartRequestParams(text=text)
        )
        await self._transport.send_line(request.model_dump_json())
        async for event in self.events():
            yield event

    # ── Bidirectional control methods ────────────────────────────────

    async def approve(
        self, request_id: str, decision: ApprovalDecision
    ) -> None:
        """Resolve a pending approval request."""
        request = ApprovalResolveRequest(
            params=ApprovalResolveRequest.ApprovalResolveRequestParams(
                request_id=request_id,
                decision=decision,
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def respond_to_question(
        self, request_id: str, response: Any
    ) -> None:
        """Respond to a user input request (AskUserQuestion tool)."""
        request = UserInputResolveRequest(
            params=UserInputResolveRequest.UserInputResolveRequestParams(
                request_id=request_id,
                response=response,
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def interrupt(self) -> None:
        """Interrupt the current turn."""
        request = TurnInterruptRequest(
            params=TurnInterruptRequest.TurnInterruptRequestParams()
        )
        await self._transport.send_line(request.model_dump_json())

    async def set_model(self, model: str) -> None:
        """Change the model for subsequent turns."""
        request = SetModelRequest(
            params=SetModelRequest.SetModelRequestParams(model=model)
        )
        await self._transport.send_line(request.model_dump_json())

    async def set_permission_mode(self, mode: str) -> None:
        """Change the permission mode."""
        request = SetPermissionModeRequest(
            params=SetPermissionModeRequest.SetPermissionModeRequestParams(
                mode=mode
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def stop_task(self, task_id: str) -> None:
        """Stop a running background task."""
        request = StopTaskRequest(
            params=StopTaskRequest.StopTaskRequestParams(task_id=task_id)
        )
        await self._transport.send_line(request.model_dump_json())

    async def update_env(self, env: dict[str, str]) -> None:
        """Update environment variables for the session."""
        request = UpdateEnvRequest(
            params=UpdateEnvRequest.UpdateEnvRequestParams(env=env)
        )
        await self._transport.send_line(request.model_dump_json())

    async def set_thinking(
        self, mode: str = "adaptive", max_tokens: int | None = None
    ) -> None:
        """Change thinking configuration for subsequent turns.

        Args:
            mode: "adaptive", "enabled", or "disabled"
            max_tokens: Maximum thinking tokens (for "enabled" mode)
        """
        thinking = {"mode": mode}
        if max_tokens is not None:
            thinking["max_tokens"] = max_tokens
        request = SetThinkingRequest(
            params=SetThinkingRequest.SetThinkingRequestParams(thinking=thinking)
        )
        await self._transport.send_line(request.model_dump_json())

    async def rewind_files(self, turn_id: str) -> None:
        """Rewind file changes to a previous turn's state."""
        request = RewindFilesRequest(
            params=RewindFilesRequest.RewindFilesRequestParams(turn_id=turn_id)
        )
        await self._transport.send_line(request.model_dump_json())

    async def cancel_request(self, request_id: str) -> None:
        """Cancel a pending server-initiated request (hook callback, approval)."""
        request = CancelRequest(
            params=CancelRequest.CancelRequestParams(request_id=request_id)
        )
        await self._transport.send_line(request.model_dump_json())

    async def list_sessions(
        self, limit: int | None = None, cursor: str | None = None
    ) -> dict[str, Any]:
        """List saved sessions. Returns raw response dict."""
        request = SessionListRequest(
            params=SessionListRequest.SessionListRequestParams(
                limit=limit, cursor=cursor
            )
        )
        await self._transport.send_line(request.model_dump_json())
        async for line_data in self._transport.read_lines():
            if line_data.get("id") is not None:
                return line_data.get("result", {})
        return {}

    async def read_session(self, session_id: str) -> dict[str, Any]:
        """Read a session's items by ID (without resuming). Returns raw response dict."""
        request = SessionReadRequest(
            params=SessionReadRequest.SessionReadRequestParams(
                session_id=session_id
            )
        )
        await self._transport.send_line(request.model_dump_json())
        async for line_data in self._transport.read_lines():
            if line_data.get("id") is not None:
                return line_data.get("result", {})
        return {}

    async def archive_session(self, session_id: str) -> None:
        """Archive a session."""
        request = SessionArchiveRequest(
            params=SessionArchiveRequest.SessionArchiveRequestParams(
                session_id=session_id
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def read_config(self, key: str | None = None) -> dict[str, Any]:
        """Read effective configuration. Returns raw config dict."""
        request = ConfigReadRequest(
            params=ConfigReadRequest.ConfigReadRequestParams(key=key)
        )
        await self._transport.send_line(request.model_dump_json())
        async for line_data in self._transport.read_lines():
            if line_data.get("id") is not None:
                return line_data.get("result", {})
        return {}

    async def write_config(
        self, key: str, value: Any, scope: str = "user"
    ) -> None:
        """Write a single configuration value."""
        request = ConfigWriteRequest(
            params=ConfigWriteRequest.ConfigWriteRequestParams(
                key=key, value=value, scope=scope
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def keep_alive(self, timestamp: int | None = None) -> None:
        """Send a keepalive signal to prevent idle timeouts."""
        from coco_sdk.generated.protocol import KeepAliveRequest

        params: dict[str, Any] = {}
        if timestamp is not None:
            params["timestamp"] = timestamp
        request = KeepAliveRequest(
            params=KeepAliveRequest.KeepAliveRequestParams(**params)
        )
        await self._transport.send_line(request.model_dump_json())

    async def _respond_to_mcp_route(
        self,
        request_id: str,
        response: Any = None,
        error: str | None = None,
    ) -> None:
        """Respond to an mcp/routeMessage server request."""
        msg = {
            "method": "mcp/routeMessageResponse",
            "params": {
                "request_id": request_id,
                "response": response if response is not None else {},
            },
        }
        if error is not None:
            msg["params"]["error"] = error
        await self._transport.send_line(json.dumps(msg))

    async def respond_to_hook(
        self, request_id: str, output: Any = None, error: str | None = None
    ) -> None:
        """Respond to an SDK hook callback."""
        request = HookCallbackResponseRequest(
            params=HookCallbackResponseRequest.HookCallbackResponseRequestParams(
                request_id=request_id,
                output=output,
                error=error,
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def resume(
        self, session_id: str, prompt: str | None = None
    ) -> AsyncIterator[ServerNotification]:
        """Resume an existing session by ID and yield events."""
        request = SessionResumeRequest(
            params=SessionResumeRequest.SessionResumeRequestParams(
                session_id=session_id,
                prompt=prompt,
            )
        )
        await self._transport.send_line(request.model_dump_json())
        async for event in self.events():
            yield event

    # ── Hook handler registration ──────────────────────────────────

    def on_hook(self, callback_id: str, handler: HookHandler) -> None:
        """Register a hook callback handler.

        When a ``hook/callback`` server request arrives with a matching
        ``callback_id``, the handler is called and the result is sent
        back automatically.
        """
        self._hook_handlers[callback_id] = handler

    # ── Convenience helpers ──────────────────────────────────────

    async def stream_text(self) -> AsyncIterator[str]:
        """Yield only text deltas from the current turn."""
        async for event in self.events():
            if event.method == "agentMessage/delta":
                delta = event.as_agent_message_delta()
                if delta:
                    yield delta.delta

    async def wait_for_turn_completed(self) -> TurnCompletedParams | None:
        """Consume all events and return the turn completion params."""
        async for event in self.events():
            tc = event.as_turn_completed()
            if tc:
                return tc
        return None

    async def get_final_text(self) -> str:
        """Consume all events and return the accumulated assistant text."""
        parts: list[str] = []
        async for event in self.events():
            if event.method == "agentMessage/delta":
                parts.append(event.params.get("delta", ""))
        return "".join(parts)

    async def close(self) -> None:
        """Close the session."""
        if self._started:
            await self._transport.close()
            self._started = False
