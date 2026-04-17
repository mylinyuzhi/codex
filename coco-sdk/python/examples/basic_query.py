"""Basic one-shot query example."""

import asyncio

from coco_sdk import query


async def main():
    async for event in query("What is 2 + 2?", max_turns=1):
        if event.method == "agentMessage/delta":
            delta = event.params.get("delta", "")
            print(delta, end="", flush=True)
        elif event.method == "turn/completed":
            print("\n--- Turn completed ---")
        elif event.method == "error":
            print(f"\nError: {event.params.get('message', 'unknown')}")


if __name__ == "__main__":
    asyncio.run(main())
