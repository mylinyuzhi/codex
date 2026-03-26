"""Convenience decorators for hooks.

The ``@hook()`` decorator auto-generates ``HookCallbackConfig`` and
registers the handler, providing zero-boilerplate hook registration::

    from cocode_sdk import hook, CocodeClient

    @hook(event="PreToolUse", matcher="Bash")
    async def block_dangerous(callback_id, event_type, input):
        if "rm -rf" in input.get("tool_input", {}).get("command", ""):
            return {"behavior": "deny", "message": "Blocked dangerous command"}
        return {"behavior": "allow"}

    async with CocodeClient(
        prompt="...",
        hooks=[block_dangerous.config],
    ) as client:
        client.on_hook(block_dangerous.callback_id, block_dangerous)
        async for event in client.events():
            print(event.method)
"""

from __future__ import annotations

import uuid
from typing import Any, Callable, Awaitable

from cocode_sdk.generated.protocol import HookCallbackConfig


class HookDefinition:
    """A decorated hook handler with pre-generated callback config."""

    def __init__(
        self,
        fn: Callable[[str, str, dict[str, Any]], Awaitable[dict[str, Any]]],
        *,
        event: str,
        matcher: str | None = None,
        timeout_ms: int | None = None,
    ):
        self.fn = fn
        self.callback_id = uuid.uuid4().hex
        self.config = HookCallbackConfig(
            callback_id=self.callback_id,
            event=event,
            matcher=matcher,
            timeout_ms=timeout_ms,
        )

    async def __call__(
        self, callback_id: str, event_type: str, input: dict[str, Any]
    ) -> dict[str, Any]:
        return await self.fn(callback_id, event_type, input)


def hook(
    *,
    event: str,
    matcher: str | None = None,
    timeout_ms: int | None = None,
) -> Callable[..., HookDefinition]:
    """Decorator to create a hook handler with auto-generated config.

    Args:
        event: Hook event type (e.g., "PreToolUse", "PostToolUse").
        matcher: Tool name pattern to match (glob-style, None = all).
        timeout_ms: Timeout for the callback response.
    """

    def decorator(fn: Callable[..., Any]) -> HookDefinition:
        return HookDefinition(fn, event=event, matcher=matcher, timeout_ms=timeout_ms)

    return decorator
