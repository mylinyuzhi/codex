"""Example: Defining custom agents with AgentDefinitionConfig."""

import asyncio

from cocode_sdk import AgentDefinitionConfig, CocodeClient


async def main():
    # Define a custom "researcher" agent that only uses read-only tools
    researcher = AgentDefinitionConfig(
        description="Research agent for reading code and searching",
        prompt="You are a code researcher. Only read and search, never modify.",
        tools=["Read", "Glob", "Grep", "WebSearch"],
        model="haiku",
        max_turns=5,
    )

    async with CocodeClient(
        prompt="Use the researcher agent to find all TODO comments in the codebase",
        agents={"researcher": researcher},
    ) as client:
        async for event in client.events():
            if event.method == "agentMessage/delta":
                delta = event.as_agent_message_delta()
                if delta:
                    print(delta.delta, end="", flush=True)
            elif event.method == "subagent/spawned":
                spawned = event.as_subagent_spawned()
                if spawned:
                    print(f"\n[Agent spawned: {spawned.agent_type}]")
            elif event.method == "subagent/completed":
                completed = event.as_subagent_completed()
                if completed:
                    print(f"\n[Agent completed: {completed.result[:100]}...]")
            elif event.method == "turn/completed":
                print()


if __name__ == "__main__":
    asyncio.run(main())
