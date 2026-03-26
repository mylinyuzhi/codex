"""Transport abstraction for communicating with the cocode binary."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Any, AsyncIterator

from cocode_sdk.generated.protocol import ServerNotification


class Transport(ABC):
    """Abstract transport for cocode SDK communication."""

    @abstractmethod
    async def start(self) -> None:
        """Start the transport (e.g., spawn subprocess)."""

    @abstractmethod
    async def send_line(self, line: str) -> None:
        """Send a JSON line to the server."""

    @abstractmethod
    async def read_events(self) -> AsyncIterator[ServerNotification]:
        """Yield server events as typed notifications."""
        ...

    @abstractmethod
    async def read_lines(self) -> AsyncIterator[dict[str, Any]]:
        """Yield raw JSON dicts from the server.

        Used by the client to distinguish ServerRequest from
        ServerNotification before parsing.
        """
        ...

    @abstractmethod
    async def close(self) -> None:
        """Shut down the transport."""
