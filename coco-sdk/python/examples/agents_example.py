"""Example: Defining custom agents with AgentDefinitionConfig."""

import asyncio

from coco_sdk import AgentDefinitionConfig, CocoClient, NotificationMethod


async def main():
    # Define a custom "researcher" agent that only uses read-only tools
    researcher = AgentDefinitionConfig(
        description="Research agent for reading code and searching",
        prompt="You are a code researcher. Only read and search, never modify.",
        tools=["Read", "Glob", "Grep", "WebSearch"],
        model="haiku",
        max_turns=5,
    )

    async with CocoClient(
        prompt="Use the researcher agent to find all TODO comments in the codebase",
        agents={"researcher": researcher},
    ) as client:
        async for event in client.events():
            if event.method == NotificationMethod.AGENT_MESSAGE_DELTA:
                delta = event.as_agent_message_delta()
                if delta:
                    print(delta.delta, end="", flush=True)
            elif event.method == NotificationMethod.SUBAGENT_SPAWNED:
                spawned = event.as_subagent_spawned()
                if spawned:
                    print(f"\n[Agent spawned: {spawned.agent_type}]")
            elif event.method == NotificationMethod.SUBAGENT_COMPLETED:
                completed = event.as_subagent_completed()
                if completed:
                    print(f"\n[Agent completed: {completed.result[:100]}...]")
            elif event.method == NotificationMethod.TURN_COMPLETED:
                print()


if __name__ == "__main__":
    asyncio.run(main())
