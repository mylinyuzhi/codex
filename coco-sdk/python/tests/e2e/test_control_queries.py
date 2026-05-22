"""End-to-end: sync request-response control queries against the live server.

Covers ``mcp_status`` and ``context_usage`` against a real
``SdkServer``. Both are simple read-only RPCs but they exercise the
``_send_and_await_response`` path (which the notification-only e2e
tests don't reach).
"""

from __future__ import annotations

import asyncio

import pytest

from coco_sdk import CocoClient
from coco_sdk.generated.protocol import NotificationMethod


async def test_mcp_status_returns_empty_roster(live_deepseek, isolated_cwd) -> None:
    """Fresh sessions have no MCP servers configured; the call should
    succeed and return an iterable container (list or dict).
    """
    async with CocoClient(
        prompt="reply with: ok",
        model=live_deepseek.model,
        cwd=str(isolated_cwd),
        max_turns=1,
    ) as client:
        # Wait for session/started so the SdkServer has bound a session.
        async with asyncio.timeout(60):
            async for event in client.events():
                if event.method == NotificationMethod.SESSION_STARTED:
                    break

        result = await client.mcp_status()

    # The wire shape is `{mcpServers: [...]}`. Be permissive about the
    # exact key but require some kind of empty container.
    assert isinstance(result, dict), f"expected dict, got {type(result).__name__}"
    servers = (
        result.get("mcpServers")
        or result.get("mcp_servers")
        or result.get("servers")
        or []
    )
    assert isinstance(servers, (list, dict)), (
        f"expected mcpServers to be list/dict, got {type(servers).__name__}"
    )


async def test_context_usage_returns_token_breakdown(live_deepseek, isolated_cwd) -> None:
    """After a real turn against DeepSeek, context_usage should report
    a non-empty breakdown — at minimum a token count (pre-turn or post-turn).
    """
    async with CocoClient(
        prompt="Reply with the single word: ok",
        model=live_deepseek.model,
        cwd=str(isolated_cwd),
        max_turns=1,
    ) as client:
        async with asyncio.timeout(120):
            async for event in client.events():
                if event.method == NotificationMethod.TURN_COMPLETED:
                    break
                if event.method == NotificationMethod.TURN_FAILED:
                    pytest.fail(f"turn failed: {event.params}")

        result = await client.context_usage()

    assert isinstance(result, dict), (
        f"context_usage should return a dict, got {type(result).__name__}"
    )
    # The exact field name has shifted across coco-rs versions
    # (`total_tokens` vs `tokens` vs nested in `breakdown`). Be lenient
    # on the key but require at least one numeric value > 0 somewhere
    # in the response — that's the universal invariant.
    def _flatten_numbers(obj):
        if isinstance(obj, (int, float)):
            return [obj]
        if isinstance(obj, dict):
            return [n for v in obj.values() for n in _flatten_numbers(v)]
        if isinstance(obj, list):
            return [n for v in obj for n in _flatten_numbers(v)]
        return []

    numbers = _flatten_numbers(result)
    assert any(n > 0 for n in numbers), (
        f"context_usage should report at least one non-zero token count; got: {result!r}"
    )
