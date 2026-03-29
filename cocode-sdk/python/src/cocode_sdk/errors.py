"""Error types for the cocode SDK."""

from __future__ import annotations


class CocodeSDKError(Exception):
    """Base exception for all cocode SDK errors."""


class CLINotFoundError(CocodeSDKError):
    """cocode binary not found on PATH or common install locations."""


class CLIConnectionError(CocodeSDKError):
    """Unable to connect to the cocode binary."""


class ProcessError(CocodeSDKError):
    """CLI process exited with a non-zero exit code.

    Attributes:
        exit_code: The process exit code.
        stderr: Captured stderr output (if available).
    """

    def __init__(
        self,
        message: str,
        *,
        exit_code: int | None = None,
        stderr: str | None = None,
    ):
        super().__init__(message)
        self.exit_code = exit_code
        self.stderr = stderr


class JSONDecodeError(CocodeSDKError):
    """Failed to decode JSON from the CLI output stream."""

    def __init__(self, message: str, *, raw_line: str | None = None):
        super().__init__(message)
        self.raw_line = raw_line


class TransportClosedError(CocodeSDKError):
    """Transport was closed unexpectedly (e.g., stdin EOF)."""


class SessionNotFoundError(CocodeSDKError):
    """Referenced session does not exist."""

    def __init__(self, session_id: str):
        super().__init__(f"Session not found: {session_id}")
        self.session_id = session_id
