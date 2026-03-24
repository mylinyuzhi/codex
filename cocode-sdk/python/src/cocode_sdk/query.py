"""One-shot query function for simple, stateless usage."""

from __future__ import annotations

from typing import AsyncIterator

from cocode_sdk._internal.transport.subprocess_cli import SubprocessCLITransport
from cocode_sdk.generated.protocol import ServerNotification, SessionStartRequest


async def query(
    prompt: str,
    *,
    model: str | None = None,
    max_turns: int | None = None,
    cwd: str | None = None,
    system_prompt_suffix: str | None = None,
    permission_mode: str | None = None,
    env: dict[str, str] | None = None,
    binary_path: str | None = None,
) -> AsyncIterator[ServerNotification]:
    """Run a single prompt and yield streaming events.

    This is the simplest way to use the SDK — fire-and-forget.

    Example::

        import asyncio
        from cocode_sdk import query

        async def main():
            async for event in query("List all Python files"):
                if event.method == "agentMessage/delta":
                    print(event.params.get("delta", ""), end="")
                elif event.method == "turn/completed":
                    print()  # Done

        asyncio.run(main())
    """
    transport = SubprocessCLITransport(
        binary_path=binary_path,
        cwd=cwd,
        env=env,
    )

    try:
        await transport.start()

        # Send session/start
        request = SessionStartRequest(
            params=SessionStartRequest.SessionStartRequestParams(
                prompt=prompt,
                model=model,
                max_turns=max_turns or 1,
                cwd=cwd,
                system_prompt_suffix=system_prompt_suffix,
                permission_mode=permission_mode,
                env=env,
            )
        )
        await transport.send_line(request.model_dump_json())

        # Yield all events until done
        async for event in transport.read_events():
            yield event
    finally:
        await transport.close()
