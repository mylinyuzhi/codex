"""Verify Python protocol types stay in sync with Rust schema exports.

Checks that all types exported in __init__.py are importable and that
key enum/model fields match the Rust protocol definitions.
"""

import cocode_sdk
from cocode_sdk.generated.protocol import (
    HookBehavior,
    HookCallbackOutput,
    PostToolUseHookInput,
    PreToolUseHookInput,
    SessionEndedParams,
    SessionEndedReason,
)


def test_all_exports_importable():
    """Every name in __all__ should be an importable attribute."""
    for name in cocode_sdk.__all__:
        assert hasattr(cocode_sdk, name), f"{name} listed in __all__ but not importable"


def test_session_ended_reason_variants():
    """SessionEndedReason should match Rust enum variants."""
    expected = {"completed", "max_turns", "max_budget", "error", "user_interrupt", "stdin_closed"}
    actual = {v.value for v in SessionEndedReason}
    assert actual == expected


def test_session_ended_params_uses_enum():
    """SessionEndedParams.reason should be SessionEndedReason, not str."""
    params = SessionEndedParams(reason="max_turns")
    assert params.reason == SessionEndedReason.max_turns


def test_hook_behavior_variants():
    expected = {"allow", "deny", "error"}
    actual = {v.value for v in HookBehavior}
    assert actual == expected


def test_pre_tool_use_hook_input():
    inp = PreToolUseHookInput(tool_name="Bash", tool_input={"command": "ls"})
    assert inp.tool_name == "Bash"
    assert inp.tool_use_id is None


def test_post_tool_use_hook_input():
    inp = PostToolUseHookInput(
        tool_name="Read",
        tool_input={"path": "/tmp/x"},
        tool_output="contents",
        is_error=False,
    )
    assert inp.tool_name == "Read"
    assert inp.tool_output == "contents"
    assert not inp.is_error


def test_hook_callback_output():
    out = HookCallbackOutput(behavior=HookBehavior.deny, message="blocked")
    assert out.behavior == HookBehavior.deny
    assert out.message == "blocked"
    assert out.updated_input is None
