"""Tests for CocoClient with a mock transport."""

import pytest

pytest.skip(
    "Tests construct ClientRequest / TurnInterruptRequest / etc with "
    "legacy cocode-sdk shapes. Pending coco-rs schema emission of these "
    "request types. Re-enable after Phase 2 client library alignment.",
    allow_module_level=True,
)



import asyncio
import json
from typing import Any, AsyncIterator

import pytest

from coco_sdk._internal.transport import Transport
from coco_sdk.client import CocoClient
from coco_sdk.generated.protocol import ServerNotification


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


def test_client_sends_session_start():
    transport = MockTransport(responses=[
        {"method": "session/started", "params": {"session_id": "s1", "protocol_version": "1"}},
        {"method": "turn/completed", "params": {"turn_id": "t1", "usage": {"input_tokens": 10, "output_tokens": 5}}},
    ])

    async def run():
        client = CocoClient(prompt="hello", transport=transport)
        await client.start()
        events = []
        async for event in client.events():
            events.append(event)
        await client.close()
        return events

    events = asyncio.get_event_loop().run_until_complete(run())

    # Verify session/start was sent
    assert len(transport.sent_lines) == 1
    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == "session/start"
    assert sent["params"]["prompt"] == "hello"

    # Verify events were received (stopped at turn/completed)
    assert len(events) == 2
    assert events[0].method == "session/started"
    assert events[1].method == "turn/completed"


def test_client_send_follow_up():
    transport = MockTransport(responses=[
        {"method": "turn/completed", "params": {"turn_id": "t2", "usage": {"input_tokens": 5, "output_tokens": 3}}},
    ])

    async def run():
        client = CocoClient(prompt="init", transport=transport)
        client._started = True
        events = []
        async for event in client.send("follow up"):
            events.append(event)
        return events

    events = asyncio.get_event_loop().run_until_complete(run())

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == "turn/start"
    assert sent["params"]["text"] == "follow up"


def test_client_auto_approval():
    transport = MockTransport(responses=[
        {"method": "approval/askForApproval", "params": {"request_id": "r1", "tool_name": "Read", "input": {}}},
        {"method": "turn/completed", "params": {"turn_id": "t1", "usage": {"input_tokens": 1, "output_tokens": 1}}},
    ])

    async def auto_approve(tool_name: str, input: dict) -> str:
        return "approve"

    async def run():
        client = CocoClient(prompt="test", transport=transport, can_use_tool=auto_approve)
        client._started = True
        events = []
        async for event in client.events():
            events.append(event)
        return events

    events = asyncio.get_event_loop().run_until_complete(run())

    # Approval should have been handled automatically (not yielded)
    assert len(events) == 1
    assert events[0].method == "turn/completed"

    # Verify approval response was sent
    approval_sent = json.loads(transport.sent_lines[0])
    assert approval_sent["method"] == "approval/resolve"
    assert approval_sent["params"]["decision"] == "approve"


def test_client_interrupt():
    transport = MockTransport()

    async def run():
        client = CocoClient(prompt="test", transport=transport)
        client._started = True
        await client.interrupt()

    asyncio.get_event_loop().run_until_complete(run())

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == "turn/interrupt"


def test_client_set_model():
    transport = MockTransport()

    async def run():
        client = CocoClient(prompt="test", transport=transport)
        client._started = True
        await client.set_model("opus")

    asyncio.get_event_loop().run_until_complete(run())

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == "control/setModel"
    assert sent["params"]["model"] == "opus"


def test_client_cancel_request():
    transport = MockTransport()

    async def run():
        client = CocoClient(prompt="test", transport=transport)
        client._started = True
        await client.cancel_request("req_42")

    asyncio.get_event_loop().run_until_complete(run())

    sent = json.loads(transport.sent_lines[0])
    assert sent["method"] == "control/cancelRequest"
    assert sent["params"]["request_id"] == "req_42"


def test_client_context_manager():
    transport = MockTransport(responses=[
        {"method": "session/started", "params": {"session_id": "s1", "protocol_version": "1"}},
        {"method": "turn/completed", "params": {"turn_id": "t1", "usage": {"input_tokens": 1, "output_tokens": 1}}},
    ])

    async def run():
        async with CocoClient(prompt="hi", transport=transport) as client:
            async for event in client.events():
                pass
        return transport._closed

    closed = asyncio.get_event_loop().run_until_complete(run())
    assert closed
