"""Multi-turn coco client for interactive sessions."""

from __future__ import annotations

import json
import logging
from typing import TYPE_CHECKING, Any, AsyncIterator, Awaitable, Callable

logger = logging.getLogger(__name__)

if TYPE_CHECKING:
    from coco_sdk.tools import ToolDefinition

from coco_sdk._internal.transport import Transport
from coco_sdk._internal.transport.subprocess_cli import SubprocessCLITransport
from coco_sdk.generated.protocol import (
    ApprovalDecision,
    ApprovalResolveRequest,
    CancelRequest,
    ClientRequestMethod,
    ConfigApplyFlagsRequest,
    ConfigReadRequest,
    ConfigWriteRequest,
    ContextUsageRequest,
    ElicitationResolveRequest,
    HookCallbackMatcher,
    HookCallbackResponseRequest,
    InitializeRequest,
    KeepAliveRequest,
    McpReconnectRequest,
    McpServerConfig,
    McpSetServersRequest,
    McpStatusRequest,
    McpToggleRequest,
    NotificationMethod,
    PermissionMode,
    PluginReloadRequest,
    RewindFilesRequest,
    ServerNotification,
    ServerRequestMethod,
    SessionArchiveRequest,
    SessionListRequest,
    SessionReadRequest,
    SessionResumeRequest,
    SessionStartRequest,
    SetModelRequest,
    SetPermissionModeRequest,
    SetThinkingRequest,
    StopTaskRequest,
    ThinkingLevel,
    TurnCompletedParams,
    TurnInterruptRequest,
    TurnStartRequest,
    UpdateEnvRequest,
    UserInputResolveRequest,
)
from coco_sdk.decorators import HookDefinition
from coco_sdk.types import ModelSpec


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

    On ``start()`` the client sends an ``initialize`` request to the
    Rust ``coco sdk`` process (registering hooks / agents / SDK-hosted
    MCP servers) and then a ``session/start`` request that carries the
    initial prompt and the per-session knobs (model, max turns, budget,
    permission mode, system prompts).

    Example::

        from coco_sdk import CocoClient
        from coco_sdk.types import DEEPSEEK

        async with CocoClient(prompt="Fix the bug in main.rs",
                              model=DEEPSEEK.flash_openai) as client:
            async for event in client.events():
                print(event.method, event.params)
    """

    def __init__(
        self,
        prompt: str,
        *,
        # Model selection
        model: str | ModelSpec | None = None,
        # Per-session knobs (mapped to SessionStartParams)
        max_turns: int | None = None,
        max_budget_usd: float | None = None,
        cwd: str | None = None,
        permission_mode: PermissionMode | str | None = None,
        system_prompt: str | None = None,
        append_system_prompt: str | None = None,
        # Initialize-time registrations.
        # `agents` is opaque on the wire (`InitializeParams.agents:
        # dict[str, Any]`), so the SDK passes user-built dicts through
        # untouched. `hooks` takes :class:`HookDefinition` instances
        # produced by ``@hook(...)`` — the wire shape
        # (:class:`HookCallbackMatcher`, keyed by event) is built in
        # :meth:`_send_initialize`.
        agents: dict[str, dict[str, Any]] | None = None,
        hooks: list[HookDefinition] | None = None,
        mcp_servers: dict[str, McpServerConfig] | None = None,
        tools: list["ToolDefinition"] | None = None,
        json_schema: dict[str, Any] | None = None,
        agent_progress_summaries: bool | None = None,
        prompt_suggestions: bool | None = None,
        # Bidirectional callbacks
        can_use_tool: CanUseTool | None = None,
        # Transport
        env: dict[str, str] | None = None,
        binary_path: str | None = None,
        transport: Transport | None = None,
    ):
        self._initial_prompt = prompt
        self._model = str(model) if model is not None else None
        self._max_turns = max_turns
        self._max_budget_usd = max_budget_usd
        self._cwd = cwd
        self._permission_mode = (
            PermissionMode(permission_mode)
            if isinstance(permission_mode, str)
            else permission_mode
        )
        self._system_prompt = system_prompt
        self._append_system_prompt = append_system_prompt
        self._agents = agents
        self._hooks = hooks
        self._mcp_servers = mcp_servers
        self._tools = tools
        self._json_schema = json_schema
        self._agent_progress_summaries = agent_progress_summaries
        self._prompt_suggestions = prompt_suggestions
        # `coco sdk` rejects the legacy default model at startup, so
        # `--model provider/model_id` must be set BEFORE the subcommand
        # rather than only sent on the wire via `session/start.model`.
        cli_args: list[str] = []
        if self._model:
            cli_args += ["--model", self._model]
        self._transport = transport or SubprocessCLITransport(
            binary_path=binary_path,
            cwd=cwd,
            env=env,
            cli_args=cli_args,
        )
        self._can_use_tool = can_use_tool
        self._hook_handlers: dict[str, HookHandler] = {}
        self._tool_registry: dict[str, "ToolDefinition"] = {}
        self._started = False

        if tools:
            for tool_def in tools:
                self._tool_registry[tool_def.server_name] = tool_def
        if hooks:
            for h in hooks:
                handler = getattr(h, "fn", None)
                cb_id = getattr(h, "callback_id", None)
                if handler and cb_id:
                    self._hook_handlers[cb_id] = handler

    async def __aenter__(self) -> "CocoClient":
        await self.start()
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    async def start(self) -> None:
        """Bring up the session: ``initialize`` → ``session/start`` → ``turn/start``.

        Three wire requests in sequence:

        1. ``initialize`` — register hooks/agents/SDK MCP servers/JSON
           schema with coco-rs.
        2. ``session/start`` — create the session shell (returns a
           ``session_id``). Does NOT run a turn — ``initial_prompt``
           on this request is metadata, not an instruction.
        3. ``turn/start`` — actually run the user's prompt and start
           the notification stream the caller iterates over.
        """
        await self._transport.start()
        self._started = True

        await self._send_initialize()
        await self._send_session_start()
        await self._send_turn_start(self._initial_prompt)

    async def _send_initialize(self) -> None:
        """Send the initialize handshake.

        Registers hooks, agents, SDK-hosted MCP servers, structured
        output schema, and system-prompt overrides. Skipped if there
        is nothing to register.
        """
        sdk_mcp_servers: list[str] = []
        if self._tools:
            for tool_def in self._tools:
                sdk_mcp_servers.append(tool_def.server_name)

        hooks_map: dict[str, list[HookCallbackMatcher]] | None = None
        if self._hooks:
            hooks_map = {}
            for h in self._hooks:
                event = getattr(h, "event", None)
                cb_id = getattr(h, "callback_id", None)
                if event is None or cb_id is None:
                    continue
                matcher = HookCallbackMatcher(
                    hook_callback_ids=[cb_id],
                    matcher=getattr(h, "matcher", None),
                    timeout=_ms_to_seconds(getattr(h, "timeout_ms", None)),
                )
                hooks_map.setdefault(event, []).append(matcher)

        # `agents` is opaque pass-through; user supplies dicts already
        # in the shape coco-rs expects. No conversion needed.
        agents_map = self._agents or None

        params = InitializeRequest.InitializeRequestParams(
            agents=agents_map,
            hooks=hooks_map,
            sdk_mcp_servers=sdk_mcp_servers or None,
            system_prompt=self._system_prompt,
            append_system_prompt=self._append_system_prompt,
            json_schema=self._json_schema,
            agent_progress_summaries=self._agent_progress_summaries,
            prompt_suggestions=self._prompt_suggestions,
        )

        request = InitializeRequest(params=params)
        await self._transport.send_request(request)

    async def _send_session_start(self) -> None:
        # `initial_prompt` is intentionally omitted — it does not
        # auto-run a turn (verified empirically against `coco sdk`).
        # The actual prompt goes through `_send_turn_start`.
        params = SessionStartRequest.SessionStartRequestParams(
            model=self._model,
            max_turns=self._max_turns,
            max_budget_usd=self._max_budget_usd,
            cwd=self._cwd,
            permission_mode=self._permission_mode,
            system_prompt=self._system_prompt,
            append_system_prompt=self._append_system_prompt,
        )
        request = SessionStartRequest(params=params)
        await self._transport.send_request(request)

    async def _send_turn_start(self, prompt: str) -> None:
        request = TurnStartRequest(
            params=TurnStartRequest.TurnStartRequestParams(prompt=prompt)
        )
        await self._transport.send_request(request)

    async def events(self) -> AsyncIterator[ServerNotification]:
        """Yield events from the current turn.

        Auto-handles ``approval/askForApproval`` if a ``can_use_tool``
        callback is provided, ``hook/callback`` if a matching handler
        is registered, and ``mcp/routeMessage`` for SDK-hosted tools.
        Other ``ServerRequest`` messages are yielded for manual handling.

        Wire-frame routing by ``type`` discriminator:

        * ``"notification"`` — yielded as :class:`ServerNotification`
          (after dispatching ``hook/callback`` / ``mcp/routeMessage``
          to registered handlers when applicable).
        * ``"request"`` — server-initiated; routes by ``method`` to the
          approval / hook / MCP / user-input handlers.
        * ``"response"`` — silently dropped (the request/reply machinery
          consumes these via :meth:`_send_and_await_response`).
        * ``"error"`` — logged at WARNING and dropped; coco-rs already
          surfaces protocol errors via the dispatcher's stderr log.
        """
        async for line_data in self._transport.read_lines():
            msg_type = line_data.get("type", "")
            if msg_type == "response":
                continue
            if msg_type == "error":
                logger.warning(
                    "wire error from coco: code=%s message=%s",
                    line_data.get("code"),
                    line_data.get("message"),
                )
                continue
            method = line_data.get("method", "")

            if method == ServerRequestMethod.APPROVAL_ASK_FOR_APPROVAL:
                if self._can_use_tool:
                    params = line_data.get("params", {})
                    decision = await self._can_use_tool(
                        params.get("tool_name", ""),
                        params.get("input", {}),
                    )
                    await self.approve(params.get("request_id", ""), decision)
                    continue
                yield _safe_parse_notification(line_data)
                continue

            if method == ServerRequestMethod.HOOK_CALLBACK:
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
                        logger.warning("Hook handler %s raised: %s", cb_id, exc)
                        await self.respond_to_hook(cb_id, output={"behavior": "allow"})
                    else:
                        if not isinstance(output, dict):
                            logger.warning(
                                "Hook handler %s returned non-dict %s; sending allow",
                                cb_id, type(output).__name__,
                            )
                            await self.respond_to_hook(cb_id, output={"behavior": "allow"})
                        else:
                            await self.respond_to_hook(cb_id, output=output)
                    continue
                yield _safe_parse_notification(line_data)
                continue

            if method == ServerRequestMethod.MCP_ROUTE_MESSAGE:
                params = line_data.get("params", {})
                server_name = params.get("server_name", "")
                request_id = params.get("request_id", "")
                message = params.get("message", {})
                tool_def = self._tool_registry.get(server_name)
                if tool_def and message.get("method") == "tools/call":
                    mcp_params = message.get("params", {})
                    msg_id = message.get("id")
                    try:
                        result = await tool_def.invoke(mcp_params.get("arguments", {}))
                        result_text = result if isinstance(result, str) else json.dumps(result)
                        response_message = {
                            "jsonrpc": "2.0",
                            "id": msg_id,
                            "result": {
                                "content": [{"type": "text", "text": result_text}],
                            },
                        }
                    except Exception as exc:
                        response_message = {
                            "jsonrpc": "2.0",
                            "id": msg_id,
                            "error": {"code": -32603, "message": str(exc)},
                        }
                    await self._respond_to_mcp_route(request_id, response_message)
                    continue
                yield _safe_parse_notification(line_data)
                continue

            if method == ServerRequestMethod.INPUT_REQUEST_USER_INPUT:
                yield _safe_parse_notification(line_data)
                continue

            event = _safe_parse_notification(line_data)
            yield event

            if method in (NotificationMethod.TURN_COMPLETED, NotificationMethod.TURN_FAILED):
                break

    async def send(self, text: str) -> AsyncIterator[ServerNotification]:
        """Send a follow-up message and yield events from the new turn."""
        request = TurnStartRequest(
            params=TurnStartRequest.TurnStartRequestParams(prompt=text)
        )
        await self._transport.send_request(request)
        async for event in self.events():
            yield event

    # ── Bidirectional control methods ────────────────────────────────

    async def approve(
        self,
        request_id: str,
        decision: ApprovalDecision,
        *,
        feedback: str | None = None,
        permission_update: Any = None,
        updated_input: Any = None,
    ) -> None:
        """Resolve a pending approval request.

        ``feedback`` surfaces a short reason to the agent.
        ``updated_input`` lets the SDK rewrite the tool call before it
        runs (e.g. tighten a glob pattern). ``permission_update`` adds a
        new permission rule to one of the four scopes
        (``user``/``project``/``local``/``session``).
        """
        params = ApprovalResolveRequest.ApprovalResolveRequestParams(
            request_id=request_id,
            decision=decision,
            feedback=feedback,
            permission_update=permission_update,
            updated_input=updated_input,
        )
        request = ApprovalResolveRequest(params=params)
        await self._transport.send_request(request)

    async def respond_to_question(
        self, request_id: str, answer: str
    ) -> None:
        """Respond to a user-input request (AskUserQuestion)."""
        request = UserInputResolveRequest(
            params=UserInputResolveRequest.UserInputResolveRequestParams(
                request_id=request_id,
                answer=answer,
            )
        )
        await self._transport.send_request(request)

    async def resolve_elicitation(
        self,
        request_id: str,
        mcp_server_name: str,
        approved: bool,
        values: dict[str, Any] | None = None,
    ) -> None:
        """Resolve an MCP elicitation form (e.g. OAuth credentials)."""
        request = ElicitationResolveRequest(
            params=ElicitationResolveRequest.ElicitationResolveRequestParams(
                request_id=request_id,
                mcp_server_name=mcp_server_name,
                approved=approved,
                values=values or {},
            )
        )
        await self._transport.send_request(request)

    async def interrupt(self) -> None:
        """Interrupt the current turn."""
        request = TurnInterruptRequest(
            params=TurnInterruptRequest.TurnInterruptRequestParams()
        )
        await self._transport.send_request(request)

    async def set_model(self, model: str | ModelSpec) -> None:
        """Change the model for subsequent turns."""
        request = SetModelRequest(
            params=SetModelRequest.SetModelRequestParams(model=str(model))
        )
        await self._transport.send_request(request)

    async def set_permission_mode(self, mode: PermissionMode | str) -> None:
        """Change the permission mode."""
        if isinstance(mode, str):
            mode = PermissionMode(mode)
        request = SetPermissionModeRequest(
            params=SetPermissionModeRequest.SetPermissionModeRequestParams(mode=mode)
        )
        await self._transport.send_request(request)

    async def set_thinking(self, level: ThinkingLevel | None) -> None:
        """Change the reasoning level for subsequent turns.

        Pass ``None`` to clear (server-side default applies). Use
        :func:`coco_sdk.types.thinking` to build the level.
        """
        request = SetThinkingRequest(
            params=SetThinkingRequest.SetThinkingRequestParams(thinking_level=level)
        )
        await self._transport.send_request(request)

    async def stop_task(self, task_id: str) -> None:
        """Stop a running background task."""
        request = StopTaskRequest(
            params=StopTaskRequest.StopTaskRequestParams(task_id=task_id)
        )
        await self._transport.send_request(request)

    async def update_env(self, env: dict[str, str]) -> None:
        """Update environment variables exposed to tool execution."""
        request = UpdateEnvRequest(
            params=UpdateEnvRequest.UpdateEnvRequestParams(env=env)
        )
        await self._transport.send_request(request)

    async def rewind_files(
        self, user_message_id: str, *, dry_run: bool = False
    ) -> None:
        """Revert files to the state at a prior user message.

        Set ``dry_run=True`` to receive a preview notification without
        touching the filesystem.
        """
        request = RewindFilesRequest(
            params=RewindFilesRequest.RewindFilesRequestParams(
                user_message_id=user_message_id,
                dry_run=dry_run,
            )
        )
        await self._transport.send_request(request)

    async def cancel_request(
        self, request_id: str, *, reason: str | None = None
    ) -> None:
        """Cancel a pending server-initiated request."""
        request = CancelRequest(
            params=CancelRequest.CancelRequestParams(
                request_id=request_id, reason=reason
            )
        )
        await self._transport.send_request(request)

    async def keep_alive(self, timestamp: int | None = None) -> None:
        """Send a keepalive signal to prevent idle timeouts."""
        params: dict[str, Any] = {}
        if timestamp is not None:
            params["timestamp"] = timestamp
        request = KeepAliveRequest(
            params=KeepAliveRequest.KeepAliveRequestParams(**params)
        )
        await self._transport.send_request(request)

    async def respond_to_hook(
        self, callback_id: str, *, output: Any = None
    ) -> None:
        """Reply to an SDK hook callback.

        ``callback_id`` matches the registered hook (``HookDefinition.callback_id``).
        ``output`` is the hook-specific decision payload (e.g. ``{"behavior": "allow"}``).
        """
        request = HookCallbackResponseRequest(
            params=HookCallbackResponseRequest.HookCallbackResponseRequestParams(
                callback_id=callback_id,
                output=output if output is not None else {},
            )
        )
        await self._transport.send_request(request)

    async def _respond_to_mcp_route(
        self, request_id: str, message: Any
    ) -> None:
        """Respond to an mcp/routeMessage server request with a JSON-RPC reply."""
        msg = {
            "method": ClientRequestMethod.MCP_ROUTE_MESSAGE_RESPONSE.value,
            "params": {
                "request_id": request_id,
                "message": message if message is not None else {},
            },
        }
        await self._transport.send_line(json.dumps(msg))

    # ── Session management ───────────────────────────────────────────

    async def list_sessions(
        self, limit: int | None = None, cursor: str | None = None
    ) -> dict[str, Any]:
        """List saved sessions. Returns the raw response dict."""
        request = SessionListRequest(
            params=SessionListRequest.SessionListRequestParams(
                limit=limit, cursor=cursor
            )
        )
        return await self._send_and_await_response(request)

    async def read_session(self, session_id: str) -> dict[str, Any]:
        """Read a session's items by ID without resuming."""
        request = SessionReadRequest(
            params=SessionReadRequest.SessionReadRequestParams(
                session_id=session_id
            )
        )
        return await self._send_and_await_response(request)

    async def archive_session(self, session_id: str) -> None:
        """Archive a session."""
        request = SessionArchiveRequest(
            params=SessionArchiveRequest.SessionArchiveRequestParams(
                session_id=session_id
            )
        )
        await self._transport.send_request(request)

    async def resume(
        self, session_id: str
    ) -> AsyncIterator[ServerNotification]:
        """Resume an existing session by ID and yield events."""
        request = SessionResumeRequest(
            params=SessionResumeRequest.SessionResumeRequestParams(
                session_id=session_id,
            )
        )
        await self._transport.send_request(request)
        async for event in self.events():
            yield event

    # ── Config ───────────────────────────────────────────────────────

    async def read_config(self) -> dict[str, Any]:
        """Read the merged effective configuration."""
        request = ConfigReadRequest(
            params=ConfigReadRequest.ConfigReadRequestParams()
        )
        return await self._send_and_await_response(request)

    async def write_config(
        self, key: str, value: Any, *, scope: str | None = None
    ) -> None:
        """Write a single configuration value."""
        request = ConfigWriteRequest(
            params=ConfigWriteRequest.ConfigWriteRequestParams(
                key=key, value=value, scope=scope
            )
        )
        await self._transport.send_request(request)

    async def apply_config_flags(self, settings: dict[str, Any]) -> None:
        """Apply runtime feature-flag settings."""
        request = ConfigApplyFlagsRequest(
            params=ConfigApplyFlagsRequest.ConfigApplyFlagsRequestParams(
                settings=settings
            )
        )
        await self._transport.send_request(request)

    # ── MCP / plugins / context introspection ───────────────────────

    async def mcp_status(self) -> dict[str, Any]:
        """Query the connection status of every MCP server."""
        request = McpStatusRequest(
            params=McpStatusRequest.McpStatusRequestParams()
        )
        return await self._send_and_await_response(request)

    async def mcp_set_servers(self, servers: dict[str, Any]) -> dict[str, Any]:
        """Hot-reload the MCP server roster."""
        request = McpSetServersRequest(
            params=McpSetServersRequest.McpSetServersRequestParams(servers=servers)
        )
        return await self._send_and_await_response(request)

    async def mcp_reconnect(self, server_name: str) -> dict[str, Any]:
        """Force-reconnect a single MCP server."""
        request = McpReconnectRequest(
            params=McpReconnectRequest.McpReconnectRequestParams(
                server_name=server_name
            )
        )
        return await self._send_and_await_response(request)

    async def mcp_toggle(self, server_name: str, enabled: bool) -> dict[str, Any]:
        """Enable or disable a single MCP server without reconnecting the others."""
        request = McpToggleRequest(
            params=McpToggleRequest.McpToggleRequestParams(
                server_name=server_name, enabled=enabled
            )
        )
        return await self._send_and_await_response(request)

    async def plugin_reload(self) -> dict[str, Any]:
        """Reload plugin definitions from disk."""
        request = PluginReloadRequest(
            params=PluginReloadRequest.PluginReloadRequestParams()
        )
        return await self._send_and_await_response(request)

    async def context_usage(self) -> dict[str, Any]:
        """Return the current context-window breakdown."""
        request = ContextUsageRequest(
            params=ContextUsageRequest.ContextUsageRequestParams()
        )
        return await self._send_and_await_response(request)

    # ── Hook handler registration ──────────────────────────────────

    def on_hook(self, callback_id: str, handler: HookHandler) -> None:
        """Register a hook callback handler.

        When ``hook/callback`` arrives with this ``callback_id``, the
        handler is invoked and the result is sent back automatically.
        """
        self._hook_handlers[callback_id] = handler

    # ── Convenience helpers ──────────────────────────────────────

    async def stream_text(self) -> AsyncIterator[str]:
        """Yield only text deltas from the current turn."""
        async for event in self.events():
            if event.method == NotificationMethod.AGENT_MESSAGE_DELTA:
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
            if event.method == NotificationMethod.AGENT_MESSAGE_DELTA:
                parts.append(event.params.get("delta", ""))
        return "".join(parts)

    async def close(self) -> None:
        """Close the session and the underlying transport."""
        if self._started:
            await self._transport.close()
            self._started = False

    # ── Internal helpers ─────────────────────────────────────────────

    async def _send_and_await_response(self, request: Any) -> dict[str, Any]:
        """Send a request and pluck the response with the matching request_id.

        coco-rs replies with ``{type: "response", request_id: N, result: {...}}``;
        we match by the ``request_id`` allocated in :meth:`send_request`.
        Notifications and other-id responses interleaved on the wire are
        skipped; an ``error`` frame for our id raises :class:`ProcessError`.
        """
        from coco_sdk.errors import ProcessError as _ProcessError

        request_id = await self._transport.send_request(request)
        async for line_data in self._transport.read_lines():
            msg_type = line_data.get("type")
            line_id = line_data.get("request_id")
            if msg_type == "response" and line_id == request_id:
                return line_data.get("result", {}) or {}
            if msg_type == "error" and line_id == request_id:
                raise _ProcessError(
                    f"coco rejected request {request_id}: {line_data.get('message', '')}",
                    exit_code=line_data.get("code"),
                )
        return {}


def _ms_to_seconds(value: int | None) -> int | None:
    """Convert a millisecond timeout to the integer-seconds wire format."""
    if value is None:
        return None
    return max(1, value // 1000)
