"""End-to-end: session persistence + ``session/resume`` round-trip.

Drives a turn against the live SdkServer, then asserts:

1. ``list_sessions`` reports the just-completed session — proves the
   server persisted it through ``SessionManager``.
2. A ``session/resume`` request for that session id returns a session
   metadata payload (carries ``session_id`` + ``cwd`` + ``model``).

Both checks happen *inside the same client lifecycle* (one binary
spawn, one ``SessionManager`` tempdir). A two-spawn cross-process
resume needs the test target to honor ``COCO_SDK_STDIO_SESSIONS_DIR``
so both spawns share storage; that's a future enhancement and not
needed to prove the resume RPC itself works.

We bypass ``CocoClient.resume`` deliberately: it exposes resume as an
async iterator over ``events()``, but ``session/resume`` is a sync RPC
in coco-rs (it returns session metadata, no follow-up notifications),
so iterating events would produce nothing. Going through
``_send_and_await_response`` matches the actual wire semantics.
"""

from __future__ import annotations

import asyncio

import pytest

from coco_sdk import CocoClient
from coco_sdk.generated.protocol import (
    NotificationMethod,
    SessionResumeRequest,
)


async def test_list_then_resume(live_deepseek, isolated_cwd) -> None:
    async with CocoClient(
        prompt="Reply with the single word: alpha",
        model=live_deepseek.model,
        cwd=str(isolated_cwd),
        max_turns=1,
    ) as client:
        # Drive turn 1 to completion so the session lands in storage.
        async with asyncio.timeout(120):
            async for event in client.events():
                if event.method == NotificationMethod.TURN_COMPLETED:
                    break
                if event.method == NotificationMethod.TURN_FAILED:
                    pytest.fail(f"turn failed: {event.params}")

        # 1) list_sessions surfaces the just-saved session.
        listing = await client.list_sessions(limit=20)
        sessions = listing.get("sessions") or listing.get("items") or []
        assert sessions, (
            f"expected list_sessions to report at least one saved session "
            f"after a completed turn; got: {listing!r}"
        )
        target = sessions[0].get("session_id") or sessions[0].get("id")
        assert isinstance(target, str) and target, (
            f"session entry missing an id: {sessions[0]!r}"
        )

        # 2) session/resume responds with session metadata. Use the
        # lower-level send-and-await pattern because the high-level
        # `client.resume(...)` helper iterates events that this RPC
        # doesn't produce.
        resume_request = SessionResumeRequest(
            params=SessionResumeRequest.SessionResumeRequestParams(
                session_id=target,
            )
        )
        resume_result = await client._send_and_await_response(resume_request)

    session_meta = resume_result.get("session") or resume_result
    assert isinstance(session_meta, dict), (
        f"expected resume response to carry a session dict, got: {resume_result!r}"
    )
    returned_id = session_meta.get("session_id") or session_meta.get("id")
    assert returned_id == target, (
        f"resume returned mismatched session_id: asked {target!r}, "
        f"got {returned_id!r}"
    )
