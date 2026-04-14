"""Tests for the @hook() decorator."""

import asyncio

from coco_sdk.decorators import HookDefinition, hook


def test_hook_decorator_creates_definition():
    @hook(event="PreToolUse", matcher="Bash")
    async def block_rm(callback_id, event_type, input):
        return {"behavior": "allow"}

    assert isinstance(block_rm, HookDefinition)
    assert block_rm.config.event == "PreToolUse"
    assert block_rm.config.matcher == "Bash"
    assert block_rm.config.callback_id  # UUID generated


def test_hook_decorator_no_matcher():
    @hook(event="PostToolUse")
    async def log_all(callback_id, event_type, input):
        return {}

    assert log_all.config.matcher is None
    assert log_all.config.event == "PostToolUse"


def test_hook_decorator_with_timeout():
    @hook(event="PreToolUse", timeout_ms=5000)
    async def slow_hook(callback_id, event_type, input):
        return {"behavior": "allow"}

    assert slow_hook.config.timeout_ms == 5000


def test_hook_unique_callback_ids():
    @hook(event="PreToolUse")
    async def hook_a(cb_id, et, inp):
        return {}

    @hook(event="PreToolUse")
    async def hook_b(cb_id, et, inp):
        return {}

    assert hook_a.config.callback_id != hook_b.config.callback_id


def test_hook_callable():
    @hook(event="PreToolUse")
    async def my_hook(callback_id, event_type, input):
        return {"behavior": "deny", "message": "blocked"}

    result = asyncio.get_event_loop().run_until_complete(
        my_hook("cb_1", "PreToolUse", {"tool_name": "Bash"})
    )
    assert result["behavior"] == "deny"


def test_hook_config_serialization():
    @hook(event="PreToolUse", matcher="Write", timeout_ms=3000)
    async def my_hook(cb_id, et, inp):
        return {}

    data = my_hook.config.model_dump(exclude_none=True)
    assert data["event"] == "PreToolUse"
    assert data["matcher"] == "Write"
    assert data["timeout_ms"] == 3000
    assert "callback_id" in data
