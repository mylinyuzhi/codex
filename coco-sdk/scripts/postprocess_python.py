#!/usr/bin/env python3
"""Generate Python Pydantic protocol types from JSON Schema.

Reads the JSON Schema files produced by the Rust `export-app-server-schema`
binary and generates a single `protocol.py` with ergonomic Pydantic models.

This replaces `datamodel-code-generator` which cannot handle our tagged-union
patterns (`serde(tag = "method", content = "params")`).

Usage:
    python postprocess_python.py <schema_dir> <output_file>

    schema_dir: Path to coco-rs/app-server-protocol/schema/json/
    output_file: Path to generated protocol.py
"""

from __future__ import annotations

import json
import re
import sys
import textwrap
from pathlib import Path

# NOTIFICATION_ACCESSORS, SERVER_REQUEST_ACCESSORS, and CLIENT_REQUEST_WRAPPERS
# are derived from the JSON schema at generation time. The schema — emitted
# from the Rust `wire_tagged_enum!` macro — is the single source of truth
# for every wire method, so no hand-maintained dict can drift.

THREAD_ITEM_ACCESSORS = {
    "agent_message": ("as_agent_message", "AgentMessageItem"),
    "reasoning": ("as_reasoning", "ReasoningItem"),
    "command_execution": ("as_command_execution", "CommandExecutionItem"),
    "file_change": ("as_file_change", "FileChangeItem"),
    "mcp_tool_call": ("as_mcp_tool_call", "McpToolCallItem"),
    "web_search": ("as_web_search", "WebSearchItem"),
    "subagent": ("as_subagent", "SubagentItem"),
    "tool_call": ("as_tool_call", "GenericToolCallItem"),
    "error": ("as_error_item", "ErrorItem"),
}

# Rename map: Rust type name -> Python type name. Applied when the Rust-side
# name would collide with an existing Python class (e.g. TurnInterruptedParams
# vs ClientRequest's TurnInterrupt) or where a pre-existing public API uses
# a different name.
TYPE_RENAMES = {
    "TurnInterruptedParams": "TurnInterruptedNotifParams",
    "KeepAliveParams": "KeepAliveNotifParams",
}

# Types that should be generated as str Enums (not object models)
ENUM_TYPES = set()  # auto-detected from schema

# Types that are referenced but should NOT be generated separately
# (they're embedded in tagged unions)
SKIP_TYPES = {
    "ThreadItemDetails",
}


def resolve_ref(ref: str) -> str:
    """Extract type name from $ref."""
    return ref.rsplit("/", 1)[-1]


def schema_to_python_type(prop: dict, required: bool, defs: dict) -> str:
    """Convert a JSON schema property to a Python type annotation."""
    if isinstance(prop, bool):
        return "Any"

    if "$ref" in prop:
        name = resolve_ref(prop["$ref"])
        name = TYPE_RENAMES.get(name, name)
        return name

    # Handle allOf with a single $ref (common schemars pattern for required refs)
    all_of = prop.get("allOf")
    if all_of and len(all_of) == 1 and "$ref" in all_of[0]:
        name = resolve_ref(all_of[0]["$ref"])
        name = TYPE_RENAMES.get(name, name)
        return name

    # Handle anyOf [$ref, null] — nullable reference (schemars Option<T>)
    any_of = prop.get("anyOf")
    if any_of:
        non_null = [v for v in any_of if v != {"type": "null"}]
        has_null = any(v == {"type": "null"} for v in any_of)
        if len(non_null) == 1:
            base = schema_to_python_type(non_null[0], True, defs)
            return f"{base} | None" if has_null else base
        # Multi-type anyOf
        types = [schema_to_python_type(v, True, defs) for v in any_of if v != {"type": "null"}]
        result = " | ".join(types)
        if has_null:
            result += " | None"
        return result

    t = prop.get("type")

    # Handle nullable types: {"type": ["integer", "null"]}
    if isinstance(t, list):
        non_null = [x for x in t if x != "null"]
        is_nullable = "null" in t
        if len(non_null) == 1:
            base = schema_to_python_type({**prop, "type": non_null[0]}, required, defs)
            return f"{base} | None" if is_nullable else base
        return "Any"

    if "oneOf" in prop and all(
        v.get("type") == "string" and "enum" in v for v in prop["oneOf"]
    ):
        # Inline enum — this shouldn't happen at property level, but handle it
        return "str"

    if "oneOf" in prop:
        # Union type
        types = []
        for variant in prop["oneOf"]:
            types.append(schema_to_python_type(variant, True, defs))
        return " | ".join(types)

    if t == "string":
        return "str"
    if t == "integer":
        return "int"
    if t == "number":
        return "float"
    if t == "boolean":
        return "bool"
    if t == "array":
        items = prop.get("items", {})
        if isinstance(items, bool) or not items:
            return "list[Any]"
        inner = schema_to_python_type(items, True, defs)
        return f"list[{inner}]"
    if t == "object":
        addl = prop.get("additionalProperties")
        if addl:
            if isinstance(addl, bool):
                return "dict[str, Any]"
            val_type = schema_to_python_type(addl, True, defs)
            return f"dict[str, {val_type}]"
        return "dict[str, Any]"

    return "Any"


def make_optional(py_type: str) -> str:
    """Wrap type in Optional."""
    if py_type == "Any":
        return "Any"
    return f"{py_type} | None"


def generate_enum(name: str, schema: dict) -> str:
    """Generate a str Enum class from either
    - `{oneOf: [{type: string, enum: [v]}]}` schemars-tagged variants, or
    - `{type: string, enum: [v, ...]}` flat string enums.
    """
    py_name = TYPE_RENAMES.get(name, name)
    lines = [f"class {py_name}(str, Enum):"]
    if "oneOf" in schema:
        for variant in schema["oneOf"]:
            value = variant["enum"][0]
            ident = value.replace("/", "_").replace("-", "_")
            lines.append(f"    {ident} = {value!r}")
    else:
        for value in schema.get("enum", []):
            ident = str(value).replace("/", "_").replace("-", "_")
            lines.append(f"    {ident} = {value!r}")
    return "\n".join(lines)


def generate_model(name: str, schema: dict, all_defs: dict) -> str:
    """Generate a BaseModel class from an object schema."""
    py_name = TYPE_RENAMES.get(name, name)
    required_fields = set(schema.get("required", []))
    properties = schema.get("properties", {})

    lines = [f"class {py_name}(BaseModel):"]

    if not properties:
        lines.append("    pass")
        return "\n".join(lines)

    # Fields that shadow BaseModel attributes and need aliases
    # Python reserved / soft-keyword aliases: field_name -> python_name.
    # Pydantic re-serializes via the `Field(alias=...)` round-trip so the
    # wire name is preserved.
    FIELD_ALIASES = {"schema": "schema_", "from": "from_"}

    # Sort: required fields first, then optional
    req_props = [(k, v) for k, v in properties.items() if k in required_fields]
    opt_props = [(k, v) for k, v in properties.items() if k not in required_fields]

    for field_name, prop in req_props + opt_props:
        is_required = field_name in required_fields
        py_type = schema_to_python_type(prop, is_required, all_defs)
        py_field_name = FIELD_ALIASES.get(field_name, field_name)
        alias_annotation = ""
        if py_field_name != field_name:
            alias_annotation = f' = Field(alias={field_name!r})'

        # If the type already includes "| None" (from nullable schema),
        # it needs a default even if required
        is_nullable = "| None" in py_type

        # Boolean schema (true = any value) has no dict methods
        prop_dict = prop if isinstance(prop, dict) else {}

        if alias_annotation:
            # Field with alias
            lines.append(f"    {py_field_name}: {py_type}{alias_annotation}")
        elif not is_required:
            default = prop_dict.get("default")
            if py_type == "bool":
                default_str = str(default) if default is not None else "False"
                lines.append(f"    {py_field_name}: {py_type} = {default_str}")
            elif default is not None:
                if isinstance(default, str):
                    lines.append(f"    {py_field_name}: {py_type} = {default!r}")
                elif isinstance(default, list):
                    lines.append(f"    {py_field_name}: {py_type} = []")
                elif isinstance(default, dict):
                    lines.append(f"    {py_field_name}: {py_type} = {{}}")
                else:
                    lines.append(f"    {py_field_name}: {py_type} = {default}")
            else:
                if not is_nullable:
                    py_type = make_optional(py_type)
                lines.append(f"    {py_field_name}: {py_type} = None")
        elif is_nullable:
            lines.append(f"    {py_field_name}: {py_type} = None")
        else:
            lines.append(f"    {py_field_name}: {py_type}")

    return "\n".join(lines)


def generate_tagged_union(
    name: str,
    doc: str,
    tag_field: str,
    params_field: str | None,
    accessors: dict[str, tuple[str, str]],
) -> str:
    """Generate a tagged union class with accessor methods."""
    lines = [f'class {name}(BaseModel):']
    lines.append(f'    """{doc}"""')
    lines.append("")
    lines.append(f"    {tag_field}: str")
    if params_field:
        lines.append(f"    {params_field}: dict[str, Any] = {{}}")

    for key, (method_name, return_type) in accessors.items():
        lines.append("")
        if params_field:
            lines.append(f"    def {method_name}(self) -> {return_type} | None:")
            lines.append(f"        if self.{tag_field} == {key!r}:")
            lines.append(f"            return {return_type}.model_validate(self.{params_field})")
            lines.append("        return None")
        else:
            # ThreadItem uses model_extra
            lines.append(f"    def {method_name}(self) -> {return_type} | None:")
            lines.append(f"        if self.{tag_field} == {key!r}:")
            lines.append(f"            return {return_type}.model_validate(self.model_extra or {{}})")
            lines.append("        return None")

    return "\n".join(lines)


def generate_thread_item() -> str:
    """Generate the ThreadItem class with extra-fields pattern."""
    lines = ['class ThreadItem(BaseModel):']
    lines.append('    """A discrete operation within a turn."""')
    lines.append("")
    lines.append("    id: str")
    lines.append("    type: str")
    lines.append('    model_config = {"extra": "allow"}')

    for key, (method_name, return_type) in THREAD_ITEM_ACCESSORS.items():
        lines.append("")
        lines.append(f"    def {method_name}(self) -> {return_type} | None:")
        lines.append(f"        if self.type == {key!r}:")
        lines.append(f"            return {return_type}.model_validate(self.model_extra or {{}})")
        lines.append("        return None")

    return "\n".join(lines)


def generate_client_request_wrapper(
    method: str, class_name: str, params_type: str
) -> str:
    """Generate a ClientRequest wrapper class."""
    lines = [f"class {class_name}(BaseModel):"]
    lines.append(f"    method: str = {method!r}")
    lines.append(f"    params: {class_name}Params")
    lines.append("")
    # Nested params class alias (re-exported at module level)
    lines.append(f"    class {class_name}Params({params_type}):")
    lines.append("        pass")
    return "\n".join(lines)


def is_enum_schema(schema: dict) -> bool:
    """Check if a schema defines a string enum.

    Handles both the schemars-tagged form (`oneOf: [{type: string, enum: [v]}]`)
    used for variant-typed enums and the plain form (`type: string, enum: [...]`)
    used for simple closed vocabularies like `ReasoningEffort`.
    """
    if "oneOf" in schema:
        return all(
            v.get("type") == "string" and "enum" in v for v in schema["oneOf"]
        )
    return schema.get("type") == "string" and "enum" in schema


def collect_definitions(schema_dir: Path) -> dict[str, dict]:
    """Collect all type definitions from all schema files."""
    defs: dict[str, dict] = {}
    # Read individual schemas first (more accurate)
    for path in sorted(schema_dir.glob("*.json")):
        if path.name == "coco_app_server_protocol.schemas.json":
            continue
        with open(path) as f:
            schema = json.load(f)
        for name, defn in schema.get("definitions", {}).items():
            if name not in defs:
                defs[name] = defn

    # Also read the bundle for types only defined there
    # (e.g., hook input/output types, standalone config types).
    # Each bundle definition is a complete root schema for that type.
    bundle_path = schema_dir / "coco_app_server_protocol.schemas.json"
    if bundle_path.exists():
        with open(bundle_path) as f:
            bundle = json.load(f)
        for name, entry in bundle.get("definitions", {}).items():
            if name in defs:
                continue
            # Entry is a full root schema (has properties/oneOf/etc.)
            if entry.get("type") == "object" or "oneOf" in entry:
                defs[name] = entry

    # Extract inline item types from ThreadItem oneOf variants.
    # These types (AgentMessageItem, ReasoningItem, etc.) are defined
    # inline in the schema, not in the definitions section.
    thread_item_path = schema_dir / "thread_item.json"
    if thread_item_path.exists():
        with open(thread_item_path) as f:
            ti_schema = json.load(f)
        for variant in ti_schema.get("oneOf", []):
            type_enum = variant.get("properties", {}).get("type", {}).get("enum", [])
            if not type_enum:
                continue
            type_val = type_enum[0]
            # Convert snake_case type to PascalCase + "Item"
            class_name = _type_to_item_class(type_val)
            if class_name and class_name not in defs:
                # Build a synthetic schema for this item type
                props = dict(variant.get("properties", {}))
                props.pop("type", None)  # Remove the discriminator
                props.pop("id", None)    # Remove the shared id field
                required = [
                    r for r in variant.get("required", [])
                    if r not in ("type", "id")
                ]
                defs[class_name] = {
                    "type": "object",
                    "properties": props,
                    "required": required,
                }
    return defs


# Map ThreadItem type discriminator values to Python class names
_ITEM_CLASS_MAP = {
    "agent_message": "AgentMessageItem",
    "reasoning": "ReasoningItem",
    "command_execution": "CommandExecutionItem",
    "file_change": "FileChangeItem",
    "mcp_tool_call": "McpToolCallItem",
    "web_search": "WebSearchItem",
    "subagent": "SubagentItem",
    "tool_call": "GenericToolCallItem",
    "error": "ErrorItem",
}


def _type_to_item_class(type_val: str) -> str | None:
    """Map a ThreadItem type discriminator to a Python class name."""
    return _ITEM_CLASS_MAP.get(type_val)


def extract_variants(schema: dict) -> list[dict]:
    """Extract full variant metadata from a wire-tagged-union schema.

    Each result dict has:
      - wire: wire-method string (e.g. "session/started")
      - description: human-readable doc from the variant
      - params_ref: `$ref`-resolved type name, or None
      - params_inline: inline params schema object, or None
      - has_params: whether the variant carries any params at all
    """
    variants: list[dict] = []
    for variant in schema.get("oneOf", []):
        props = variant.get("properties", {})
        wire = props.get("method", {}).get("enum", [None])[0]
        if not wire:
            continue
        params = props.get("params")
        entry: dict = {
            "wire": wire,
            "description": variant.get("description", ""),
            "params_ref": None,
            "params_inline": None,
            "has_params": False,
        }
        if params is None:
            pass  # unit variant
        elif "$ref" in params:
            entry["params_ref"] = resolve_ref(params["$ref"])
            entry["has_params"] = True
        elif params.get("type") == "object":
            entry["params_inline"] = params
            entry["has_params"] = True
        variants.append(entry)
    return variants


def load_schema(schema_dir: Path, name: str) -> dict:
    """Read a single top-level schema file by name (without .json)."""
    with open(schema_dir / f"{name}.json") as f:
        return json.load(f)


def extract_notification_methods(schema_dir: Path) -> list[tuple[str, str]]:
    """Legacy extractor kept for back-compat: (method, params_ref) pairs for
    variants with external `$ref` params. Prefer `extract_variants()`."""
    variants = extract_variants(load_schema(schema_dir, "server_notification"))
    return [(v["wire"], v["params_ref"]) for v in variants if v["params_ref"]]


def extract_client_request_methods(schema_dir: Path) -> list[tuple[str, str]]:
    """Legacy extractor kept for back-compat; see `extract_variants`."""
    variants = extract_variants(load_schema(schema_dir, "client_request"))
    return [(v["wire"], v["params_ref"]) for v in variants if v["params_ref"]]


def _wire_words(wire: str) -> list[str]:
    """Split a wire string into lowercase component words.

        session/stateChanged    -> ["session", "state", "changed"]
        plan_approval/requested -> ["plan", "approval", "requested"]
        rateLimit               -> ["rate", "limit"]
        error                   -> ["error"]
    """
    s = wire.replace("/", "_")
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return [part.lower() for part in s.split("_") if part]


def wire_to_enum_member(wire: str) -> str:
    """Wire string -> Python Enum member name (SCREAMING_SNAKE_CASE)."""
    return "_".join(_wire_words(wire)).upper()


def wire_to_accessor(wire: str) -> str:
    """Wire string -> `as_x_y_z` accessor method name."""
    return "as_" + "_".join(_wire_words(wire))


def _pascal_case(words: list[str]) -> str:
    return "".join(w[:1].upper() + w[1:] for w in words)


def _accessor_to_params_class(accessor: str) -> str:
    """`as_session_state_changed` -> `SessionStateChangedParams`."""
    base = accessor.removeprefix("as_")
    return _pascal_case(base.split("_")) + "Params"


def _derive_request_wrapper_name(variant: dict) -> str:
    """Derive the ClientRequest wrapper class name for a variant.

    Rule:
      1. If the variant has a `$ref` params type, strip `Params` from the
         type name and use that as the base.
      2. Otherwise (unit or inline) PascalCase the wire string with the
         leading `control/` prefix stripped.
      3. Append `Request` unless the base already ends in `Request` (avoids
         `CancelRequestRequest`).
    """
    params_ref = variant.get("params_ref")
    if params_ref:
        base = params_ref.removesuffix("Params")
    else:
        wire = variant["wire"]
        if wire.startswith("control/"):
            wire = wire[len("control/") :]
        base = _pascal_case(_wire_words(wire))
    if not base.endswith("Request"):
        base = base + "Request"
    return base


def derive_accessors(variants: list[dict]) -> dict[str, tuple[str, str]]:
    """Build `{wire: (accessor_name, params_class)}` from variant metadata.

    Variants without params still get accessor entries pointing at a
    synthesised empty Pydantic model (so the `as_x() -> Params | None`
    idiom works uniformly). Renames in `TYPE_RENAMES` are applied to the
    `$ref`-based class names.
    """
    result: dict[str, tuple[str, str]] = {}
    for v in variants:
        wire = v["wire"]
        accessor = wire_to_accessor(wire)
        if v["params_ref"]:
            params_class = TYPE_RENAMES.get(v["params_ref"], v["params_ref"])
        else:
            params_class = _accessor_to_params_class(accessor)
        result[wire] = (accessor, params_class)
    return result


def generate_inline_params_models(
    variants: list[dict],
    all_defs: dict,
    emitted: set[str],
) -> str:
    """Emit synthesized Pydantic models for inline-params / unit variants.

    `emitted` tracks class names already produced this run to avoid
    duplicates when a synthesized name collides with an existing type.
    """
    blocks: list[str] = []
    for v in variants:
        if v["params_ref"]:
            continue
        class_name = _accessor_to_params_class(wire_to_accessor(v["wire"]))
        if class_name in emitted or class_name in all_defs:
            continue
        emitted.add(class_name)
        if v["params_inline"]:
            blocks.append(generate_model(class_name, v["params_inline"], all_defs))
        else:
            # Unit variant — empty params model with `extra='allow'` so it
            # round-trips any stray fields without validation errors.
            blocks.append(
                textwrap.dedent(
                    f'''\
                    class {class_name}(BaseModel):
                        """Empty params for the wire-method `{v["wire"]}`."""

                        model_config = {{"extra": "allow"}}
                    '''
                ).rstrip()
            )
        blocks.append("")
    return "\n".join(blocks)


def _method_enum_block(
    schema_dir: Path,
    schema_name: str,
    class_name: str,
    doc: str,
) -> str:
    """Emit `class X(str, Enum)` from a method-enum schema file."""
    schema = load_schema(schema_dir, schema_name)
    lines = [f"class {class_name}(str, Enum):", f'    """{doc}"""', ""]
    for wire in schema.get("enum", []):
        lines.append(f"    {wire_to_enum_member(wire)} = {wire!r}")
    return "\n".join(lines)


def generate_notification_method_enum(schema_dir: Path) -> str:
    return _method_enum_block(
        schema_dir,
        "notification_method",
        "NotificationMethod",
        "Wire-method identifier for every `ServerNotification` variant. "
        "Mirrors the Rust `NotificationMethod` enum. Members inherit from "
        "`str`, so equality with raw wire strings Just Works.",
    )


def generate_client_request_method_enum(schema_dir: Path) -> str:
    return _method_enum_block(
        schema_dir,
        "client_request_method",
        "ClientRequestMethod",
        "Wire-method identifier for every `ClientRequest` variant. "
        "Mirrors the Rust `ClientRequestMethod` enum.",
    )


def generate_server_request_method_enum(schema_dir: Path) -> str:
    return _method_enum_block(
        schema_dir,
        "server_request_method",
        "ServerRequestMethod",
        "Wire-method identifier for every `ServerRequest` variant. "
        "Mirrors the Rust `ServerRequestMethod` enum.",
    )


def main() -> None:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <schema_dir> <output_file>", file=sys.stderr)
        sys.exit(1)

    schema_dir = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    # Collect all definitions
    all_defs = collect_definitions(schema_dir)

    # Types handled explicitly (skip from generic generation)
    explicitly_handled = {
        "ThreadItem", "ThreadItemDetails",
        "ServerNotification", "ServerRequest", "ClientRequest",
        # MCP config types handled manually
        "McpServerConfig",
    }

    # Classify types
    enum_names: set[str] = set()
    model_names: set[str] = set()
    for name, defn in all_defs.items():
        if name in SKIP_TYPES or name in explicitly_handled:
            continue
        if is_enum_schema(defn):
            enum_names.add(name)
        elif defn.get("type") == "object":
            model_names.add(name)

    ENUM_TYPES.update(enum_names)

    # Determine generation order based on dependencies
    # Generate: enums first, then models, then tagged unions, then wrappers
    sections: list[str] = []

    # Header
    sections.append(textwrap.dedent('''\
        """Generated protocol types for the coco SDK.

        These types mirror the Rust `coco-app-server-protocol` crate.
        Regenerate with: `scripts/generate_python.sh`

        Source schemas: coco-rs/app-server-protocol/schema/json/

        DO NOT EDIT MANUALLY — changes will be overwritten by the generator.
        """

        from __future__ import annotations

        from enum import Enum
        from typing import Any

        from pydantic import BaseModel, Field
    '''))

    # ── Section: Usage ──
    sections.append("# " + "-" * 75)
    sections.append("# Usage")
    sections.append("# " + "-" * 75)
    sections.append("")
    if "Usage" in all_defs:
        sections.append(generate_model("Usage", all_defs["Usage"], all_defs))
        model_names.discard("Usage")
    sections.append("")

    # ── Section: Enums ──
    sections.append("# " + "-" * 75)
    sections.append("# Enums")
    sections.append("# " + "-" * 75)
    sections.append("")
    for name in sorted(enum_names):
        sections.append(generate_enum(name, all_defs[name]))
        sections.append("")
    sections.append("")

    # ── Section: Item types ──
    item_types = [
        "AgentMessageItem", "ReasoningItem", "CommandExecutionItem",
        "FileChange", "FileChangeKind", "FileChangeItem",
        "McpToolCallResult", "McpToolCallError", "McpToolCallItem",
        "WebSearchItem", "SubagentItem", "GenericToolCallItem", "ErrorItem",
    ]
    sections.append("# " + "-" * 75)
    sections.append("# Item types")
    sections.append("# " + "-" * 75)
    sections.append("")
    for name in item_types:
        if name in all_defs:
            if name in enum_names:
                continue  # Already generated
            sections.append(generate_model(name, all_defs[name], all_defs))
            sections.append("")
            model_names.discard(name)
    sections.append("")

    # ── Section: ThreadItem ──
    sections.append("# " + "-" * 75)
    sections.append("# ThreadItem (tagged union with extra fields)")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(generate_thread_item())
    sections.append("")
    sections.append("")

    # Parse all three wire-tagged unions once so we can derive params models,
    # accessor maps, and wrapper classes from the same variant lists.
    notif_variants = extract_variants(load_schema(schema_dir, "server_notification"))
    sr_variants = extract_variants(load_schema(schema_dir, "server_request"))
    cr_variants = extract_variants(load_schema(schema_dir, "client_request"))

    notif_param_types = {v["params_ref"] for v in notif_variants if v["params_ref"]}
    sr_param_types = {v["params_ref"] for v in sr_variants if v["params_ref"]}
    emitted_synth: set[str] = set()
    generated_params: set[str] = set()

    def emit_ref_params(types: set[str]) -> None:
        for params_type in sorted(types):
            if params_type in generated_params:
                continue
            generated_params.add(params_type)
            if params_type in all_defs and params_type not in enum_names:
                sections.append(generate_model(params_type, all_defs[params_type], all_defs))
                sections.append("")
                model_names.discard(params_type)

    # ── Section: Notification params ──
    sections.append("# " + "-" * 75)
    sections.append("# Server notification params")
    sections.append("# " + "-" * 75)
    sections.append("")
    emit_ref_params(notif_param_types)
    synth = generate_inline_params_models(notif_variants, all_defs, emitted_synth)
    if synth:
        sections.append(synth)
    sections.append("")

    # ── Section: NotificationMethod enum (protocol constants) ──
    sections.append("# " + "-" * 75)
    sections.append("# Notification wire-method constants")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(generate_notification_method_enum(schema_dir))
    sections.append("")
    sections.append("")

    # ── Section: ServerNotification (tagged union with auto-derived accessors) ──
    sections.append("# " + "-" * 75)
    sections.append("# Server notifications (tagged union)")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(generate_tagged_union(
        "ServerNotification",
        "An event from the server. Use `method` to determine the event type.",
        "method",
        "params",
        derive_accessors(notif_variants),
    ))
    sections.append("")
    sections.append("")

    # ── Section: Server request params and ServerRequest ──
    sections.append("# " + "-" * 75)
    sections.append("# Server requests (server -> client, require response)")
    sections.append("# " + "-" * 75)
    sections.append("")
    emit_ref_params(sr_param_types)
    synth = generate_inline_params_models(sr_variants, all_defs, emitted_synth)
    if synth:
        sections.append(synth)
    sections.append("")
    sections.append(generate_server_request_method_enum(schema_dir))
    sections.append("")
    sections.append("")
    sections.append(generate_tagged_union(
        "ServerRequest",
        "A request from the server that requires a client response.",
        "method",
        "params",
        derive_accessors(sr_variants),
    ))
    sections.append("")
    sections.append("")

    # ── Section: MCP server config (special: tagged union with 'type' field) ──
    # These must come BEFORE client request params since SessionStartRequestParams
    # references AgentDefinitionConfig, McpServerConfig, etc.
    sections.append("# " + "-" * 75)
    sections.append("# MCP server config types")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(textwrap.dedent('''\
        class StdioMcpServerConfig(BaseModel):
            """Subprocess-based MCP server (stdio transport)."""

            type: str = "stdio"
            command: str
            args: list[str] = []
            env: dict[str, str] | None = None


        class SseMcpServerConfig(BaseModel):
            """SSE-based MCP server."""

            type: str = "sse"
            url: str


        class HttpMcpServerConfig(BaseModel):
            """HTTP-based MCP server."""

            type: str = "http"
            url: str


        McpServerConfig = StdioMcpServerConfig | SseMcpServerConfig | HttpMcpServerConfig
    '''))

    # ── Section: Remaining config types ──
    remaining_configs = [
        "AgentHookConfig", "AgentDefinitionConfig", "HookCallbackConfig",
        "SandboxConfig", "ThinkingConfig", "OutputFormatConfig",
        "SystemPromptConfig", "ToolsConfig", "HookMatcherConfig",
        "CommandInfo",
        # Initialize / session management result types
        "ClientInfo", "InitializeCapabilities",
        "SessionSummary",
    ]
    sections.append("# " + "-" * 75)
    sections.append("# Config types")
    sections.append("# " + "-" * 75)
    sections.append("")
    for name in remaining_configs:
        if name in all_defs and name not in enum_names and name in model_names:
            sections.append(generate_model(name, all_defs[name], all_defs))
            sections.append("")
            model_names.discard(name)

    # Union-type aliases (oneOf types that aren't enums or objects)
    # These need to be defined as `Any` since they're complex unions
    # (e.g., SystemPromptConfig = str | {preset, append}).
    union_types = [
        "SystemPromptConfig",
        "ToolsConfig",
        "ErrorInfo",
        # PermissionUpdate is a oneOf tagged union of action objects; no
        # Pydantic-class generation yet, fall back to `Any` so ApprovalResolve
        # request params resolve without a forward-ref error.
        "PermissionUpdate",
    ]
    for name in union_types:
        if name in all_defs and name not in model_names and name not in enum_names:
            sections.append(f"# Union type: see Rust source for variants")
            sections.append(f"{name} = Any")
            sections.append("")
    sections.append("")

    # ── Section: Hook input/output types ──
    hook_types = [
        "PreToolUseHookInput", "PostToolUseHookInput", "PostToolUseFailureHookInput",
        "HookCallbackOutput",
        "StopHookInput", "SubagentStartHookInput", "SubagentStopHookInput",
        "UserPromptSubmitHookInput", "NotificationHookInput",
        "PreCompactHookInput", "PermissionRequestHookInput",
        "SessionStartHookInput", "SessionEndHookInput",
    ]
    sections.append("# " + "-" * 75)
    sections.append("# Hook input/output types")
    sections.append("# " + "-" * 75)
    sections.append("")
    for name in hook_types:
        if name in all_defs and name not in enum_names and name in model_names:
            sections.append(generate_model(name, all_defs[name], all_defs))
            sections.append("")
            model_names.discard(name)
    sections.append("")

    # ── Section: Client request params ──
    cr_param_types = {v["params_ref"] for v in cr_variants if v["params_ref"]}
    sections.append("# " + "-" * 75)
    sections.append("# Client request params")
    sections.append("# " + "-" * 75)
    sections.append("")
    for params_type in sorted(cr_param_types):
        if params_type in generated_params:
            continue
        generated_params.add(params_type)
        if params_type in all_defs and params_type not in enum_names:
            if params_type not in notif_param_types and params_type not in sr_param_types:
                sections.append(generate_model(params_type, all_defs[params_type], all_defs))
                sections.append("")
                model_names.discard(params_type)
    sections.append("")

    # ── Section: ClientRequestMethod enum (protocol constants) ──
    sections.append("# " + "-" * 75)
    sections.append("# Client request wire-method constants")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(generate_client_request_method_enum(schema_dir))
    sections.append("")
    sections.append("")

    # ── Section: Client request wrappers (auto-derived from schema) ──
    sections.append("# " + "-" * 75)
    sections.append("# Client request wrappers")
    sections.append("# " + "-" * 75)
    sections.append("")
    for variant in cr_variants:
        method = variant["wire"]
        class_name = _derive_request_wrapper_name(variant)
        params_ref = variant.get("params_ref")
        py_params = TYPE_RENAMES.get(params_ref, params_ref) if params_ref else None
        lines = [
            f"class {class_name}(BaseModel):",
            f"    method: str = {method!r}",
            f"    params: {class_name}Params",
            "",
        ]
        if py_params:
            lines.append(f"    class {class_name}Params({py_params}):")
            lines.append("        pass")
        else:
            # Unit variant — keep the nested Params class for caller API
            # symmetry; `extra='allow'` tolerates any stray fields.
            lines.append(f"    class {class_name}Params(BaseModel):")
            lines.append('        model_config = {"extra": "allow"}')
        sections.append("\n".join(lines))
        sections.append("")
        sections.append(f"{class_name}Params = {class_name}.{class_name}Params")
        sections.append("")
    sections.append("")

    # ── Section: Remaining types (PermissionSuggestion, etc.) ──
    if model_names:
        sections.append("# " + "-" * 75)
        sections.append("# Additional types")
        sections.append("# " + "-" * 75)
        sections.append("")
        for name in sorted(model_names):
            if name in all_defs:
                sections.append(generate_model(name, all_defs[name], all_defs))
                sections.append("")

    # Schema-driven derivation is exhaustive by construction, so the prior
    # hand-maintained dicts no longer exist. Keep a lightweight summary
    # print for visibility.
    validation_errors: list[str] = []

    if validation_errors:
        # Warn, don't fail. The accessor maps are hand-written and may
        # lag the Rust schema; missing methods just mean the Python side
        # won't get convenient `as_X()` accessors for those variants,
        # but the underlying Pydantic models are still generated correctly.
        print("VALIDATION WARNINGS:", file=sys.stderr)
        for err in validation_errors:
            print(f"  - {err}", file=sys.stderr)
        print(
            "  (continuing anyway — accessor maps lag the Rust schema; "
            "update them in postprocess_python.py if you need typed "
            "accessors for the missing methods)",
            file=sys.stderr,
        )

    print(
        f"Generated: {len(notif_variants)} notifications, "
        f"{len(sr_variants)} server requests, "
        f"{len(cr_variants)} client requests"
    )

    # Write output
    content = "\n".join(sections)
    # Clean up triple+ blank lines
    while "\n\n\n\n" in content:
        content = content.replace("\n\n\n\n", "\n\n\n")
    output_path.write_text(content)
    print(f"Generated: {output_path} ({len(content)} bytes)")


if __name__ == "__main__":
    main()
