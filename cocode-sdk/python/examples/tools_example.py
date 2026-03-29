"""Example: Using @tool() to define in-process MCP tools."""

import asyncio

from cocode_sdk import CocodeClient, tool


@tool()
def get_weather(city: str) -> str:
    """Get current weather for a city."""
    # In a real app, this would call a weather API
    return f"Sunny, 22C in {city}"


@tool(name="calculate", description="Perform arithmetic")
def calculate(expression: str) -> str:
    """Evaluate a math expression."""
    try:
        result = eval(expression)  # noqa: S307
        return str(result)
    except Exception as e:
        return f"Error: {e}"


async def main():
    async with CocodeClient(
        prompt="What's the weather in Tokyo? Also, what's 42 * 17?",
        tools=[get_weather, calculate],
        permission_mode="bypassPermissions",
    ) as client:
        async for event in client.events():
            if event.method == "agentMessage/delta":
                delta = event.as_agent_message_delta()
                if delta:
                    print(delta.delta, end="", flush=True)
            elif event.method == "turn/completed":
                print()


if __name__ == "__main__":
    asyncio.run(main())
