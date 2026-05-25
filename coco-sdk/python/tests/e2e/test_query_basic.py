"""End-to-end: one-shot ``query()`` against deepseek-v4-flash.

Spawns ``coco sdk --model deepseek-openai/deepseek-v4-flash``, sends a
trivial prompt, and asserts that:

* the session starts,
* at least one assistant text delta arrives,
* the turn finishes with usage > 0.

Skips cleanly when ``DEEPSEEK_API_KEY`` is missing.
"""

from __future__ import annotations

import asyncio

import pytest

from coco_sdk import query
from coco_sdk.generated.protocol import NotificationMethod


async def test_query_basic_completes(live_deepseek, isolated_cwd) -> None:
    saw_turn_started = False
    saw_text_delta = False
    saw_turn_completed = False
    final_usage: dict | None = None

    try:
        async with asyncio.timeout(120):
            async for event in query(
                "Reply with the single word: ok",
                model=live_deepseek.model,
                cwd=str(isolated_cwd),
                max_turns=1,
            ):
                method = event.method
                if method == NotificationMethod.TURN_STARTED:
                    saw_turn_started = True
                elif method == NotificationMethod.AGENT_MESSAGE_DELTA:
                    saw_text_delta = True
                elif method == NotificationMethod.TURN_COMPLETED:
                    saw_turn_completed = True
                    completed = event.as_turn_completed()
                    if completed and completed.usage:
                        final_usage = (
                            completed.usage.model_dump()
                            if hasattr(completed.usage, "model_dump")
                            else dict(completed.usage)
                        )
                    break
                elif method == NotificationMethod.TURN_FAILED:
                    pytest.fail(f"turn failed: {event.params}")
    except asyncio.TimeoutError:
        pytest.fail("query() did not complete within 120s")

    assert saw_turn_started, "expected turn/started after sending turn/start"
    assert saw_text_delta, "expected at least one assistant text delta"
    assert saw_turn_completed, "expected turn/completed terminator"
    assert (
        final_usage and final_usage.get("output_tokens", {}).get("total", 0) > 0
    ), f"expected non-zero output tokens, got {final_usage!r}"
