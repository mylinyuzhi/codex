#!/usr/bin/env python3
"""Regenerate `coco_sdk/__init__.py` from the current protocol.py.

Scans all top-level class definitions in `generated/protocol.py` and
rewrites `__init__.py` to import every one of them. This avoids stale
imports after regeneration.

Usage: regen_init.py <protocol_path>
"""
from __future__ import annotations

import ast
import sys
from pathlib import Path


STATIC_EXPORTS = [
    "__version__",
    "query",
    "CanUseTool",
    "CocoClient",
    "HookHandler",
    "HookDefinition",
    "hook",
    "ToolDefinition",
    "tool",
    "TypedClient",
    "CLIConnectionError",
    "CLINotFoundError",
    "CocoSDKError",
    "JSONDecodeError",
    "ProcessError",
    "SessionNotFoundError",
    "TransportClosedError",
]

HEADER = '''"""coco SDK — programmatic access to the coco multi-provider LLM CLI.

Two usage patterns:

1. One-shot query (simplest)::

    from coco_sdk import query

    async for event in query("Fix the bug"):
        print(event.method, event.params)

2. Multi-turn client::

    from coco_sdk import CocoClient

    async with CocoClient(prompt="Fix the bug") as client:
        async for event in client.events():
            print(event.method)
"""

from coco_sdk.client import CanUseTool, CocoClient, HookHandler
from coco_sdk.decorators import HookDefinition, hook
from coco_sdk.errors import (
    CLIConnectionError,
    CLINotFoundError,
    CocoSDKError,
    JSONDecodeError,
    ProcessError,
    SessionNotFoundError,
    TransportClosedError,
)
from coco_sdk.query import query
from coco_sdk.structured import TypedClient
from coco_sdk.tools import ToolDefinition, tool

# Protocol types — auto-generated from coco-rs schemas.
# Regenerate with: ./coco-sdk/scripts/generate_all.sh
from coco_sdk.generated.protocol import (
'''

FOOTER_TEMPLATE = '''
__version__ = "0.1.0"

__all__ = [
{all_items}
]
'''


def main(proto_path: Path) -> int:
    try:
        tree = ast.parse(proto_path.read_text())
    except SyntaxError as e:
        print(f"error: protocol.py parse failed: {e}", file=sys.stderr)
        return 1

    class_names = sorted(
        {node.name for node in tree.body if isinstance(node, ast.ClassDef)}
    )

    init_path = proto_path.parent.parent / "__init__.py"

    parts = [HEADER]
    for name in class_names:
        parts.append(f"    {name},\n")
    parts.append(")\n")

    all_items = "\n".join(
        f'    "{name}",' for name in STATIC_EXPORTS + class_names
    )
    parts.append(FOOTER_TEMPLATE.format(all_items=all_items))

    init_path.write_text("".join(parts))
    print(
        f"  wrote {init_path.relative_to(init_path.parent.parent.parent.parent)} "
        f"({len(class_names)} protocol types + {len(STATIC_EXPORTS)} static)"
    )
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: regen_init.py <protocol_path>", file=sys.stderr)
        sys.exit(2)
    sys.exit(main(Path(sys.argv[1])))
