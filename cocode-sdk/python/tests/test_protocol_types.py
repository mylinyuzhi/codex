"""Tests for protocol type serialization."""

from cocode_sdk.generated.protocol import (
    AgentMessageDeltaParams,
    ApprovalDecision,
    CommandExecutionItem,
    ItemStatus,
    ServerNotification,
    SessionStartRequest,
    ThreadItem,
    TurnCompletedParams,
    TurnStartRequest,
    Usage,
)


def test_usage_defaults():
    usage = Usage()
    assert usage.input_tokens == 0
    assert usage.output_tokens == 0
    assert usage.cache_read_tokens is None


def test_usage_with_values():
    usage = Usage(input_tokens=100, output_tokens=50, cache_read_tokens=20)
    assert usage.input_tokens == 100
    assert usage.cache_read_tokens == 20


def test_item_status_enum():
    assert ItemStatus.in_progress == "in_progress"
    assert ItemStatus.completed == "completed"
    assert ItemStatus.failed == "failed"


def test_command_execution_item():
    item = CommandExecutionItem(
        command="ls -la",
        aggregated_output="total 0",
        exit_code=0,
        status=ItemStatus.completed,
    )
    assert item.command == "ls -la"
    assert item.exit_code == 0


def test_server_notification_roundtrip():
    notif = ServerNotification(
        method="turn/started",
        params={"turn_id": "turn_1", "turn_number": 1},
    )
    json_str = notif.model_dump_json()
    parsed = ServerNotification.model_validate_json(json_str)
    assert parsed.method == "turn/started"
    assert parsed.params["turn_id"] == "turn_1"


def test_server_notification_as_turn_completed():
    notif = ServerNotification(
        method="turn/completed",
        params={
            "turn_id": "turn_1",
            "usage": {"input_tokens": 100, "output_tokens": 50},
        },
    )
    tc = notif.as_turn_completed()
    assert tc is not None
    assert tc.turn_id == "turn_1"
    assert tc.usage.input_tokens == 100


def test_server_notification_wrong_type_returns_none():
    notif = ServerNotification(
        method="turn/started",
        params={"turn_id": "t", "turn_number": 1},
    )
    assert notif.as_turn_completed() is None
    assert notif.as_error() is None


def test_session_start_request_serialization():
    request = SessionStartRequest(
        params=SessionStartRequest.SessionStartRequestParams(
            prompt="hello",
            model="sonnet",
            max_turns=5,
        )
    )
    data = request.model_dump()
    assert data["method"] == "session/start"
    assert data["params"]["prompt"] == "hello"
    assert data["params"]["model"] == "sonnet"


def test_turn_start_request_serialization():
    request = TurnStartRequest(
        params=TurnStartRequest.TurnStartRequestParams(text="follow up")
    )
    data = request.model_dump()
    assert data["method"] == "turn/start"
    assert data["params"]["text"] == "follow up"


def test_approval_decision_enum():
    assert ApprovalDecision.approve == "approve"
    assert ApprovalDecision.deny == "deny"


def test_thread_item_with_extra_fields():
    item = ThreadItem.model_validate(
        {
            "id": "item_1",
            "type": "command_execution",
            "command": "git status",
            "aggregated_output": "clean",
            "exit_code": 0,
            "status": "completed",
        }
    )
    assert item.id == "item_1"
    assert item.type == "command_execution"
    cmd = item.as_command_execution()
    assert cmd is not None
    assert cmd.command == "git status"
    assert cmd.exit_code == 0


def test_agent_message_delta_params():
    params = AgentMessageDeltaParams(
        item_id="msg_0",
        turn_id="turn_1",
        delta="Hello ",
    )
    assert params.delta == "Hello "
