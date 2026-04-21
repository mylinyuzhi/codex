"""Tests for CocoClient with a mock transport.

All wire-method comparisons go through the typed `NotificationMethod` /
`ClientRequestMethod` / `ServerRequestMethod` enums generated from the
coco-rs schema — no raw wire strings.
"""

import asyncio
import json
from typing import Any, AsyncIterator

import pytest

from coco_sdk._internal.transport import Transport
from coco_sdk.client import CocoClient
from coco_sdk.generated.protocol import (
    ClientRequestMethod,
    NotificationMethod,
    ServerNotification,
    ServerRequestMethod,
)


class MockTransport(Transport):
    """In-memory transport for testing."""

    def __init__(self, responses: list[dict[str, Any]] | None = None):
        self.sent_lines: list[str] = []
        self._responses = responses or []
        self._started = False
        self._closed = False

    async def start(self) -> None:
        self._started = True

    async def send_line(self, line: str) -> None:
        self.sent_lines.append(line)

    async def read_lines(self) -> AsyncIterator[dict[str, Any]]:
        for resp in self._responses:
            yield resp

    async def read_events(self) -> AsyncIterator[ServerNotification]:
        for resp in self._responses:
            yield ServerNotification.model_validate(resp)

    async def close(self) -> None:
        self._closed = True


def _notif(method: NotificationMethod, **params: Any) -> dict[str, Any]:
    return {"method": method.value, "params": params}


def _server_request(method: ServerRequestMethod, **params: Any) -> dict[str, Any]:
    return {"method": method.value, "params": params}


@pytest.mark.asyncio
async def test_client_sends_session_start() -> None:
    transport = MockTransport(responses=[
        _notif(NotificationMethod.SESSION_STARTED, session_id="s1", protocol_version="1"),
        _notif(
            NotificationMethod.TURN_COMPLETED,
            turn_id="t1",
            usage={"input_tokens": 10, "output_tokens": 5},
        ),
    ])

    client = CocoClient(prompt="hello", transport=transport)
    await client.start()
    events = [event async for event in client.events()]
    await client.close()

    # Verify session/start was sent
    assert len(transport.sent_lines) == 1
    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == ClientRequestMethod.SESSION_START
    assert sent["params"]["initial_prompt"] == "hello"

    # Verify events were received (stopped at TURN_COMPLETED)
    assert len(events) == 2
    assert events[0].method == NotificationMethod.SESSION_STARTED
    assert events[1].method == NotificationMethod.TURN_COMPLETED


@pytest.mark.asyncio
async def test_client_send_follow_up() -> None:
    transport = MockTransport(responses=[
        _notif(
            NotificationMethod.TURN_COMPLETED,
            turn_id="t2",
            usage={"input_tokens": 5, "output_tokens": 3},
        ),
    ])

    client = CocoClient(prompt="init", transport=transport)
    client._started = True
    events = [event async for event in client.send("follow up")]

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == ClientRequestMethod.TURN_START
    assert sent["params"]["prompt"] == "follow up"
    assert len(events) == 1


@pytest.mark.asyncio
async def test_client_auto_approval() -> None:
    transport = MockTransport(responses=[
        _server_request(
            ServerRequestMethod.APPROVAL_ASK_FOR_APPROVAL,
            request_id="r1",
            tool_name="Read",
            input={},
        ),
        _notif(
            NotificationMethod.TURN_COMPLETED,
            turn_id="t1",
            usage={"input_tokens": 1, "output_tokens": 1},
        ),
    ])

    from coco_sdk.generated.protocol import ApprovalDecision

    async def auto_approve(tool_name: str, input: dict) -> ApprovalDecision:
        return ApprovalDecision.allow

    client = CocoClient(prompt="test", transport=transport, can_use_tool=auto_approve)
    client._started = True
    events = [event async for event in client.events()]

    # Approval handled automatically → not yielded
    assert len(events) == 1
    assert events[0].method == NotificationMethod.TURN_COMPLETED

    # Verify approval response was sent
    approval_sent = json.loads(transport.sent_lines[0])
    assert approval_sent["method"] == ClientRequestMethod.APPROVAL_RESOLVE
    assert approval_sent["params"]["decision"] == "allow"


@pytest.mark.asyncio
async def test_client_interrupt() -> None:
    transport = MockTransport()
    client = CocoClient(prompt="test", transport=transport)
    client._started = True
    await client.interrupt()

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == ClientRequestMethod.TURN_INTERRUPT


@pytest.mark.asyncio
async def test_client_set_model() -> None:
    transport = MockTransport()
    client = CocoClient(prompt="test", transport=transport)
    client._started = True
    await client.set_model("opus")

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == ClientRequestMethod.CONTROL_SET_MODEL
    assert sent["params"]["model"] == "opus"


@pytest.mark.asyncio
async def test_client_cancel_request() -> None:
    transport = MockTransport()
    client = CocoClient(prompt="test", transport=transport)
    client._started = True
    await client.cancel_request("req_42")

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == ClientRequestMethod.CONTROL_CANCEL_REQUEST
    assert sent["params"]["request_id"] == "req_42"


@pytest.mark.asyncio
async def test_client_context_manager() -> None:
    transport = MockTransport(responses=[
        _notif(NotificationMethod.SESSION_STARTED, session_id="s1", protocol_version="1"),
        _notif(
            NotificationMethod.TURN_COMPLETED,
            turn_id="t1",
            usage={"input_tokens": 1, "output_tokens": 1},
        ),
    ])

    async with CocoClient(prompt="hi", transport=transport) as client:
        _ = [event async for event in client.events()]

    assert transport._closed
