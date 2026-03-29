"""Subprocess transport — spawns the cocode binary with --sdk-mode."""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
from typing import Any, AsyncIterator

from cocode_sdk.errors import (
    CLIConnectionError,
    CLINotFoundError,
    ProcessError,
    TransportClosedError,
)
from cocode_sdk.generated.protocol import ServerNotification

from . import Transport

logger = logging.getLogger("cocode_sdk.transport")


def _find_cocode_binary() -> str:
    """Locate the cocode binary on PATH or common install locations."""
    binary = shutil.which("cocode")
    if binary:
        return binary

    candidates = [
        os.path.expanduser("~/.cargo/bin/cocode"),
        "/usr/local/bin/cocode",
    ]
    for path in candidates:
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path

    raise CLINotFoundError(
        "cocode binary not found. Install it or set COCODE_PATH environment variable."
    )


class SubprocessCLITransport(Transport):
    """Transport that spawns cocode as a subprocess with --sdk-mode.

    Communication is via NDJSON over stdin/stdout. Stderr is captured
    and logged for debugging.
    """

    MAX_START_RETRIES = 3
    INITIAL_BACKOFF = 1.0

    def __init__(
        self,
        binary_path: str | None = None,
        cwd: str | None = None,
        env: dict[str, str] | None = None,
    ):
        self._binary_path = binary_path or os.environ.get("COCODE_PATH") or _find_cocode_binary()
        self._cwd = cwd
        self._env = env
        self._process: asyncio.subprocess.Process | None = None
        self._stderr_task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        last_error: Exception | None = None
        for attempt in range(self.MAX_START_RETRIES):
            try:
                await self._start_process()
                return
            except OSError as e:
                last_error = e
                backoff = self.INITIAL_BACKOFF * (2 ** attempt)
                logger.warning(
                    "Failed to start cocode (attempt %d/%d): %s. Retrying in %.1fs",
                    attempt + 1, self.MAX_START_RETRIES, e, backoff,
                )
                await asyncio.sleep(backoff)
        raise CLIConnectionError(
            f"Failed to start cocode after {self.MAX_START_RETRIES} attempts"
        ) from last_error

    async def _start_process(self) -> None:
        cmd = [self._binary_path, "--sdk-mode"]

        process_env = os.environ.copy()
        process_env["COCODE_ENTRYPOINT"] = "sdk-py"
        if self._env:
            process_env.update(self._env)

        self._process = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self._cwd,
            env=process_env,
        )

        # Spawn background task to capture and log stderr
        self._stderr_task = asyncio.create_task(self._read_stderr())

    async def _read_stderr(self) -> None:
        """Read stderr from the subprocess and log it."""
        if not self._process or not self._process.stderr:
            return
        while True:
            line = await self._process.stderr.readline()
            if not line:
                break
            text = line.decode().rstrip()
            if text:
                logger.debug("cocode stderr: %s", text)

    async def send_line(self, line: str) -> None:
        if not self._process or not self._process.stdin:
            raise TransportClosedError("Transport not started")
        data = (line.rstrip("\n") + "\n").encode()
        self._process.stdin.write(data)
        await self._process.stdin.drain()

    async def read_lines(self) -> AsyncIterator[dict[str, Any]]:
        if not self._process or not self._process.stdout:
            raise TransportClosedError("Transport not started")

        while True:
            line = await self._process.stdout.readline()
            if not line:
                returncode = self._process.returncode
                if returncode is not None and returncode != 0:
                    raise ProcessError(
                        f"cocode process exited with code {returncode}",
                        exit_code=returncode,
                    )
                break
            line_str = line.decode().strip()
            if not line_str:
                continue
            try:
                yield json.loads(line_str)
            except json.JSONDecodeError as e:
                logger.warning("Malformed JSON from cocode: %s (line: %s)", e, line_str[:200])

    async def read_events(self) -> AsyncIterator[ServerNotification]:
        async for data in self.read_lines():
            try:
                yield ServerNotification.model_validate(data)
            except Exception as e:
                logger.warning("Failed to parse server event: %s", e)

    async def close(self) -> None:
        if self._process:
            if self._process.stdin:
                self._process.stdin.close()
            try:
                self._process.terminate()
                await asyncio.wait_for(self._process.wait(), timeout=5.0)
            except (ProcessLookupError, asyncio.TimeoutError):
                self._process.kill()
            self._process = None
        if self._stderr_task:
            self._stderr_task.cancel()
            self._stderr_task = None
