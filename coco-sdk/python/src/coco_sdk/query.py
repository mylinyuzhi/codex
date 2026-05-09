"""One-shot query function for simple, stateless usage."""

from __future__ import annotations

from typing import AsyncIterator

from coco_sdk._internal.transport.subprocess_cli import SubprocessCLITransport
from coco_sdk.generated.protocol import (
    InitializeRequest,
    PermissionMode,
    ServerNotification,
    SessionStartRequest,
    TurnStartRequest,
)
from coco_sdk.types import ModelSpec


async def query(
    prompt: str,
    *,
    model: str | ModelSpec | None = None,
    max_turns: int | None = None,
    cwd: str | None = None,
    append_system_prompt: str | None = None,
    system_prompt: str | None = None,
    permission_mode: PermissionMode | str | None = None,
    max_budget_usd: float | None = None,
    env: dict[str, str] | None = None,
    binary_path: str | None = None,
) -> AsyncIterator[ServerNotification]:
    """Run a single prompt and yield streaming events.

    The simplest way to use the SDK — fire-and-forget. For multi-turn
    sessions, hooks, or in-process tools, use :class:`CocoClient`.

    ``model`` accepts either a string in ``"<provider>/<model_id>"`` form
    or a :class:`~coco_sdk.types.ModelSpec`. ``env`` is forwarded to
    the subprocess environment (not the wire protocol — coco-rs reads
    env vars natively).

    Example::

        import asyncio
        from coco_sdk import query
        from coco_sdk.generated.protocol import NotificationMethod
        from coco_sdk.types import DEEPSEEK

        async def main():
            async for event in query("List Python files", model=DEEPSEEK.flash_openai):
                if event.method == NotificationMethod.AGENT_MESSAGE_DELTA:
                    print(event.params.get("delta", ""), end="")

        asyncio.run(main())
    """
    model_str = str(model) if model is not None else None
    # `coco sdk` rejects the legacy default model at startup, so
    # `--model provider/model_id` must be set BEFORE the subcommand
    # rather than only sent on the wire via `session/start.model`.
    cli_args: list[str] = []
    if model_str:
        cli_args += ["--model", model_str]
    transport = SubprocessCLITransport(
        binary_path=binary_path,
        cwd=cwd,
        env=env,
        cli_args=cli_args,
    )

    try:
        await transport.start()

        # 1) initialize — capability negotiation handshake.
        await transport.send_request(InitializeRequest(
            params=InitializeRequest.InitializeRequestParams()
        ))

        # 2) session/start — create the session shell. `initial_prompt`
        #    on this request does NOT auto-run a turn (it's just a label
        #    for the first user message); turns are launched separately
        #    via `turn/start`.
        await transport.send_request(SessionStartRequest(params=SessionStartRequest.SessionStartRequestParams(
            model=model_str,
            max_turns=max_turns,
            cwd=cwd,
            append_system_prompt=append_system_prompt,
            system_prompt=system_prompt,
            permission_mode=(
                PermissionMode(permission_mode)
                if isinstance(permission_mode, str)
                else permission_mode
            ),
            max_budget_usd=max_budget_usd,
        )))

        # 3) turn/start — actually runs the prompt and produces the
        #    notification stream.
        await transport.send_request(TurnStartRequest(
            params=TurnStartRequest.TurnStartRequestParams(prompt=prompt)
        ))

        async for event in transport.read_events():
            yield event
    finally:
        await transport.close()
