"""Multi-turn cocode client for interactive sessions."""

from __future__ import annotations

import json
from typing import AsyncIterator

from cocode_sdk._internal.transport import Transport
from cocode_sdk._internal.transport.subprocess_cli import SubprocessCLITransport
from cocode_sdk.generated.protocol import (
    ServerNotification,
    SessionStartRequest,
    TurnStartRequest,
)


class CocodeClient:
    """Multi-turn client for cocode sessions.

    Example::

        async with CocodeClient(prompt="Fix the bug in main.rs") as client:
            async for event in client.events():
                print(event.method, event.params)

            # Send follow-up
            async for event in client.send("Now add tests"):
                print(event.method, event.params)
    """

    def __init__(
        self,
        prompt: str,
        *,
        model: str | None = None,
        max_turns: int | None = None,
        cwd: str | None = None,
        system_prompt_suffix: str | None = None,
        permission_mode: str | None = None,
        env: dict[str, str] | None = None,
        binary_path: str | None = None,
        transport: Transport | None = None,
    ):
        self._initial_prompt = prompt
        self._model = model
        self._max_turns = max_turns
        self._cwd = cwd
        self._system_prompt_suffix = system_prompt_suffix
        self._permission_mode = permission_mode
        self._env = env
        self._transport = transport or SubprocessCLITransport(
            binary_path=binary_path,
            cwd=cwd,
            env=env,
        )
        self._started = False

    async def __aenter__(self) -> CocodeClient:
        await self.start()
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    async def start(self) -> None:
        """Start the session by sending session/start."""
        await self._transport.start()
        self._started = True

        # Send session/start request
        request = SessionStartRequest(
            params=SessionStartRequest.SessionStartRequestParams(
                prompt=self._initial_prompt,
                model=self._model,
                max_turns=self._max_turns,
                cwd=self._cwd,
                system_prompt_suffix=self._system_prompt_suffix,
                permission_mode=self._permission_mode,
                env=self._env,
            )
        )
        await self._transport.send_line(request.model_dump_json())

    async def events(self) -> AsyncIterator[ServerNotification]:
        """Yield events from the current turn."""
        async for event in self._transport.read_events():
            yield event
            # Stop after turn completion or failure
            if event.method in ("turn/completed", "turn/failed"):
                break

    async def send(self, text: str) -> AsyncIterator[ServerNotification]:
        """Send a follow-up message and yield events from the new turn."""
        request = TurnStartRequest(
            params=TurnStartRequest.TurnStartRequestParams(text=text)
        )
        await self._transport.send_line(request.model_dump_json())
        async for event in self.events():
            yield event

    async def close(self) -> None:
        """Close the session."""
        if self._started:
            await self._transport.close()
            self._started = False
