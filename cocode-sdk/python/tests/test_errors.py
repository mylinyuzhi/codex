"""Tests for error type hierarchy."""

from cocode_sdk.errors import (
    CLIConnectionError,
    CLINotFoundError,
    CocodeSDKError,
    JSONDecodeError,
    ProcessError,
    SessionNotFoundError,
    TransportClosedError,
)


def test_error_hierarchy():
    """All SDK errors inherit from CocodeSDKError."""
    assert issubclass(CLINotFoundError, CocodeSDKError)
    assert issubclass(CLIConnectionError, CocodeSDKError)
    assert issubclass(ProcessError, CocodeSDKError)
    assert issubclass(JSONDecodeError, CocodeSDKError)
    assert issubclass(TransportClosedError, CocodeSDKError)
    assert issubclass(SessionNotFoundError, CocodeSDKError)


def test_process_error_attributes():
    err = ProcessError("failed", exit_code=1, stderr="boom")
    assert err.exit_code == 1
    assert err.stderr == "boom"
    assert "failed" in str(err)


def test_json_decode_error_attributes():
    err = JSONDecodeError("bad json", raw_line="{broken")
    assert err.raw_line == "{broken"


def test_session_not_found():
    err = SessionNotFoundError("sess_123")
    assert err.session_id == "sess_123"
    assert "sess_123" in str(err)


def test_catch_base_exception():
    """CocodeSDKError catches all SDK errors."""
    try:
        raise CLINotFoundError("not found")
    except CocodeSDKError as e:
        assert "not found" in str(e)
