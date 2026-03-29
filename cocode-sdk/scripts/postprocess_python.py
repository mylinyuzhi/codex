#!/usr/bin/env python3
"""Generate Python Pydantic protocol types from JSON Schema.

Reads the JSON Schema files produced by the Rust `export-app-server-schema`
binary and generates a single `protocol.py` with ergonomic Pydantic models.

This replaces `datamodel-code-generator` which cannot handle our tagged-union
patterns (`serde(tag = "method", content = "params")`).

Usage:
    python postprocess_python.py <schema_dir> <output_file>

    schema_dir: Path to cocode-rs/app-server-protocol/schema/json/
    output_file: Path to generated protocol.py
"""

from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path

# ── Accessor maps for tagged unions ──────────────────────────────────────

NOTIFICATION_ACCESSORS = {
    # ── Session lifecycle ──────────────────────────────────────────────
    "session/started": ("as_session_started", "SessionStartedParams"),
    "session/result": ("as_session_result", "SessionResultParams"),
    "session/ended": ("as_session_ended", "SessionEndedParams"),
    # ── Turn lifecycle ─────────────────────────────────────────────────
    "turn/started": ("as_turn_started", "TurnStartedParams"),
    "turn/completed": ("as_turn_completed", "TurnCompletedParams"),
    "turn/failed": ("as_turn_failed", "TurnFailedParams"),
    "turn/interrupted": ("as_turn_interrupted", "TurnInterruptedNotifParams"),
    "turn/maxReached": ("as_max_turns_reached", "MaxTurnsReachedParams"),
    "turn/retry": ("as_turn_retry", "TurnRetryParams"),
    # ── Item lifecycle ─────────────────────────────────────────────────
    "item/started": ("as_item_started", "ItemEventParams"),
    "item/updated": ("as_item_updated", "ItemEventParams"),
    "item/completed": ("as_item_completed", "ItemEventParams"),
    # ── Content streaming ──────────────────────────────────────────────
    "agentMessage/delta": ("as_agent_message_delta", "AgentMessageDeltaParams"),
    "reasoning/delta": ("as_reasoning_delta", "ReasoningDeltaParams"),
    # ── Sub-agent events ───────────────────────────────────────────────
    "subagent/spawned": ("as_subagent_spawned", "SubagentSpawnedParams"),
    "subagent/completed": ("as_subagent_completed", "SubagentCompletedParams"),
    "subagent/backgrounded": ("as_subagent_backgrounded", "SubagentBackgroundedParams"),
    "subagent/progress": ("as_subagent_progress", "SubagentProgressParams"),
    # ── MCP events ─────────────────────────────────────────────────────
    "mcp/startupStatus": ("as_mcp_startup_status", "McpStartupStatusParams"),
    "mcp/startupComplete": ("as_mcp_startup_complete", "McpStartupCompleteParams"),
    # ── Context management ─────────────────────────────────────────────
    "context/compacted": ("as_context_compacted", "ContextCompactedParams"),
    "context/usageWarning": ("as_context_usage_warning", "ContextUsageWarningParams"),
    "context/compactionStarted": ("as_compaction_started", "CompactionStartedParams"),
    "context/compactionFailed": ("as_compaction_failed", "CompactionFailedParams"),
    "context/cleared": ("as_context_cleared", "ContextClearedParams"),
    # ── Background task events ─────────────────────────────────────────
    "task/started": ("as_task_started", "TaskStartedParams"),
    "task/completed": ("as_task_completed", "TaskCompletedParams"),
    "task/progress": ("as_task_progress", "TaskProgressParams"),
    "agents/killed": ("as_agents_killed", "AgentsKilledParams"),
    # ── Model events ───────────────────────────────────────────────────
    "model/fallbackStarted": ("as_model_fallback_started", "ModelFallbackStartedParams"),
    "model/fallbackCompleted": ("as_model_fallback_completed", "ModelFallbackCompletedParams"),
    "model/fastModeChanged": ("as_fast_mode_changed", "FastModeChangedParams"),
    # ── Permission events ──────────────────────────────────────────────
    "permission/modeChanged": ("as_permission_mode_changed", "PermissionModeChangedParams"),
    # ── Prompt suggestions ─────────────────────────────────────────────
    "prompt/suggestion": ("as_prompt_suggestion", "PromptSuggestionParams"),
    # ── System-level events ────────────────────────────────────────────
    "error": ("as_error", "ErrorNotificationParams"),
    "rateLimit": ("as_rate_limit", "RateLimitParams"),
    "keepAlive": ("as_keep_alive", "KeepAliveNotifParams"),
    # ── IDE integration events ─────────────────────────────────────────
    "ide/selectionChanged": ("as_ide_selection_changed", "IdeSelectionChangedParams"),
    "ide/diagnosticsUpdated": ("as_ide_diagnostics_updated", "IdeDiagnosticsUpdatedParams"),
    # ── Plan mode ──────────────────────────────────────────────────────
    "plan/modeChanged": ("as_plan_mode_changed", "PlanModeChangedParams"),
    # ── Queue ──────────────────────────────────────────────────────────
    "queue/stateChanged": ("as_queue_state_changed", "QueueStateChangedParams"),
    "queue/commandQueued": ("as_command_queued", "CommandQueuedParams"),
    "queue/commandDequeued": ("as_command_dequeued", "CommandDequeuedParams"),
    # ── Rewind ─────────────────────────────────────────────────────────
    "rewind/completed": ("as_rewind_completed", "RewindCompletedParams"),
    "rewind/failed": ("as_rewind_failed", "RewindFailedParams"),
    # ── Cost ───────────────────────────────────────────────────────────
    "cost/warning": ("as_cost_warning", "CostWarningParams"),
    # ── Sandbox ────────────────────────────────────────────────────────
    "sandbox/stateChanged": ("as_sandbox_state_changed", "SandboxStateChangedParams"),
    "sandbox/violationsDetected": ("as_sandbox_violations_detected", "SandboxViolationsDetectedParams"),
    # ── Agent registry ─────────────────────────────────────────────────
    "agents/registered": ("as_agents_registered", "AgentsRegisteredParams"),
    # ── Hook ───────────────────────────────────────────────────────────
    "hook/executed": ("as_hook_executed", "HookExecutedParams"),
    # ── Summarize ──────────────────────────────────────────────────────
    "summarize/completed": ("as_summarize_completed", "SummarizeCompletedParams"),
    "summarize/failed": ("as_summarize_failed", "SummarizeFailedParams"),
    # ── Stream health ──────────────────────────────────────────────────
    "stream/stallDetected": ("as_stream_stall_detected", "StreamStallDetectedParams"),
    "stream/watchdogWarning": ("as_stream_watchdog_warning", "StreamWatchdogWarningParams"),
    # ── Stream lifecycle ───────────────────────────────────────────────
    "stream/requestEnd": ("as_stream_request_end", "StreamRequestEndParams"),
}

SERVER_REQUEST_ACCESSORS = {
    "approval/askForApproval": ("as_ask_for_approval", "AskForApprovalParams"),
    "input/requestUserInput": ("as_request_user_input", "RequestUserInputParams"),
    "hook/callback": ("as_hook_callback", "HookCallbackParams"),
    "mcp/routeMessage": ("as_mcp_route_message", "McpRouteMessageParams"),
    "control/cancelRequest": ("as_cancel_request", "ServerCancelRequestParams"),
}

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

# Maps for ClientRequest wrapper generation: method -> (class_name, params_type)
CLIENT_REQUEST_WRAPPERS = {
    "initialize": ("InitializeRequest", "InitializeRequestParams"),
    "session/start": ("SessionStartRequest", "SessionStartRequestParams"),
    "session/resume": ("SessionResumeRequest", "SessionResumeRequestParams"),
    "turn/start": ("TurnStartRequest", "TurnStartRequestParams"),
    "turn/interrupt": ("TurnInterruptRequest", "TurnInterruptRequestParams"),
    "approval/resolve": ("ApprovalResolveRequest", "ApprovalResolveRequestParams"),
    "input/resolveUserInput": ("UserInputResolveRequest", "UserInputResolveRequestParams"),
    "control/setModel": ("SetModelRequest", "SetModelRequestParams"),
    "control/setPermissionMode": ("SetPermissionModeRequest", "SetPermissionModeRequestParams"),
    "control/stopTask": ("StopTaskRequest", "StopTaskRequestParams"),
    "hook/callbackResponse": ("HookCallbackResponseRequest", "HookCallbackResponseRequestParams"),
    "control/setThinking": ("SetThinkingRequest", "SetThinkingRequestParams"),
    "control/rewindFiles": ("RewindFilesRequest", "RewindFilesRequestParams"),
    "control/updateEnv": ("UpdateEnvRequest", "UpdateEnvRequestParams"),
    "control/keepAlive": ("KeepAliveRequest", "KeepAliveRequestParams"),
    "session/list": ("SessionListRequest", "SessionListRequestParams"),
    "session/read": ("SessionReadRequest", "SessionReadRequestParams"),
    "session/archive": ("SessionArchiveRequest", "SessionArchiveRequestParams"),
    "config/read": ("ConfigReadRequest", "ConfigReadRequestParams"),
    "config/value/write": ("ConfigWriteRequest", "ConfigWriteRequestParams"),
    "mcp/routeMessageResponse": ("McpRouteMessageResponseRequest", "McpRouteMessageResponseParams"),
    "control/cancelRequest": ("CancelRequest", "CancelRequestParams"),
}

# Rename map: Rust type name -> Python type name (where they differ)
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
    """Generate a str Enum class from a oneOf-of-strings schema."""
    py_name = TYPE_RENAMES.get(name, name)
    lines = [f"class {py_name}(str, Enum):"]
    for variant in schema["oneOf"]:
        value = variant["enum"][0]
        # Python identifier: replace non-alphanumeric with _
        ident = value.replace("/", "_").replace("-", "_")
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
    FIELD_ALIASES = {"schema": "schema_"}

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
    """Check if a schema defines a string enum."""
    if "oneOf" in schema:
        return all(
            v.get("type") == "string" and "enum" in v
            for v in schema["oneOf"]
        )
    return False


def collect_definitions(schema_dir: Path) -> dict[str, dict]:
    """Collect all type definitions from all schema files."""
    defs: dict[str, dict] = {}
    # Read individual schemas first (more accurate)
    for path in sorted(schema_dir.glob("*.json")):
        if path.name == "cocode_app_server_protocol.schemas.json":
            continue
        with open(path) as f:
            schema = json.load(f)
        for name, defn in schema.get("definitions", {}).items():
            if name not in defs:
                defs[name] = defn

    # Also read the bundle for types only defined there
    # (e.g., hook input/output types, standalone config types).
    # Each bundle definition is a complete root schema for that type.
    bundle_path = schema_dir / "cocode_app_server_protocol.schemas.json"
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


def extract_notification_methods(schema_dir: Path) -> list[tuple[str, str]]:
    """Extract (method, params_ref) from ServerNotification schema."""
    with open(schema_dir / "server_notification.json") as f:
        schema = json.load(f)
    result = []
    for variant in schema.get("oneOf", []):
        props = variant.get("properties", {})
        method_val = props.get("method", {}).get("enum", [None])[0]
        params_ref = props.get("params", {}).get("$ref", "")
        params_type = resolve_ref(params_ref) if params_ref else None
        if method_val and params_type:
            result.append((method_val, params_type))
    return result


def extract_client_request_methods(schema_dir: Path) -> list[tuple[str, str]]:
    """Extract (method, params_ref) from ClientRequest schema."""
    with open(schema_dir / "client_request.json") as f:
        schema = json.load(f)
    result = []
    for variant in schema.get("oneOf", []):
        props = variant.get("properties", {})
        method_val = props.get("method", {}).get("enum", [None])[0]
        params_ref = props.get("params", {}).get("$ref", "")
        params_type = resolve_ref(params_ref) if params_ref else None
        if method_val and params_type:
            result.append((method_val, params_type))
    return result


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
        """Generated protocol types for the cocode SDK.

        These types mirror the Rust `cocode-app-server-protocol` crate.
        Regenerate with: `scripts/generate_python.sh`

        Source schemas: cocode-rs/app-server-protocol/schema/json/

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

    # ── Section: Notification params ──
    notif_methods = extract_notification_methods(schema_dir)
    notif_param_types = {params for _, params in notif_methods}

    sections.append("# " + "-" * 75)
    sections.append("# Server notification params")
    sections.append("# " + "-" * 75)
    sections.append("")
    generated_params: set[str] = set()
    for _, params_type in notif_methods:
        if params_type in generated_params:
            continue
        generated_params.add(params_type)
        if params_type in all_defs and params_type not in enum_names:
            sections.append(generate_model(params_type, all_defs[params_type], all_defs))
            sections.append("")
            model_names.discard(params_type)
    sections.append("")

    # ── Section: ServerNotification ──
    sections.append("# " + "-" * 75)
    sections.append("# Server notifications (tagged union)")
    sections.append("# " + "-" * 75)
    sections.append("")
    sections.append(generate_tagged_union(
        "ServerNotification",
        "An event from the server. Use `method` to determine the event type.",
        "method",
        "params",
        NOTIFICATION_ACCESSORS,
    ))
    sections.append("")
    sections.append("")

    # ── Section: Server request params and ServerRequest ──
    sections.append("# " + "-" * 75)
    sections.append("# Server requests (server -> client, require response)")
    sections.append("# " + "-" * 75)
    sections.append("")
    sr_param_types = set()
    with open(schema_dir / "server_request.json") as f:
        sr_schema = json.load(f)
    for variant in sr_schema.get("oneOf", []):
        params_ref = variant.get("properties", {}).get("params", {}).get("$ref", "")
        params_type = resolve_ref(params_ref) if params_ref else None
        if params_type and params_type in all_defs and params_type not in enum_names:
            sections.append(generate_model(params_type, all_defs[params_type], all_defs))
            sections.append("")
            sr_param_types.add(params_type)
            model_names.discard(params_type)
    sections.append("")
    sections.append(generate_tagged_union(
        "ServerRequest",
        "A request from the server that requires a client response.",
        "method",
        "params",
        SERVER_REQUEST_ACCESSORS,
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
    # (e.g., SystemPromptConfig = str | {preset, append})
    union_types = ["SystemPromptConfig", "ToolsConfig", "ErrorInfo"]
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
    cr_methods = extract_client_request_methods(schema_dir)
    sections.append("# " + "-" * 75)
    sections.append("# Client request params")
    sections.append("# " + "-" * 75)
    sections.append("")
    for method, params_type in cr_methods:
        if params_type in generated_params:
            continue
        generated_params.add(params_type)
        if params_type in all_defs and params_type not in enum_names:
            if params_type not in notif_param_types and params_type not in sr_param_types:
                sections.append(generate_model(params_type, all_defs[params_type], all_defs))
                sections.append("")
                model_names.discard(params_type)
    sections.append("")

    # ── Section: Client request wrappers ──
    sections.append("# " + "-" * 75)
    sections.append("# Client request wrappers")
    sections.append("# " + "-" * 75)
    sections.append("")
    for method, params_type in cr_methods:
        if method in CLIENT_REQUEST_WRAPPERS:
            class_name, _ = CLIENT_REQUEST_WRAPPERS[method]
            py_params = TYPE_RENAMES.get(params_type, params_type)
            lines = [f"class {class_name}(BaseModel):"]
            lines.append(f"    method: str = {method!r}")
            lines.append(f"    params: {class_name}Params")
            lines.append("")
            lines.append(f"    class {class_name}Params({py_params}):")
            lines.append("        pass")
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

    # ── Validation: ensure accessor maps cover all schema variants ──
    validation_errors: list[str] = []

    # Validate ServerNotification coverage
    schema_notif_methods = {m for m, _ in notif_methods}
    accessor_notif_methods = set(NOTIFICATION_ACCESSORS.keys())
    missing_notif = schema_notif_methods - accessor_notif_methods
    extra_notif = accessor_notif_methods - schema_notif_methods
    if missing_notif:
        validation_errors.append(
            f"NOTIFICATION_ACCESSORS missing {len(missing_notif)} methods "
            f"from schema: {sorted(missing_notif)}"
        )
    if extra_notif:
        validation_errors.append(
            f"NOTIFICATION_ACCESSORS has {len(extra_notif)} methods "
            f"not in schema: {sorted(extra_notif)}"
        )

    # Validate ServerRequest coverage
    sr_methods_from_schema: set[str] = set()
    for variant in sr_schema.get("oneOf", []):
        method_val = variant.get("properties", {}).get("method", {}).get("enum", [None])[0]
        if method_val:
            sr_methods_from_schema.add(method_val)
    accessor_sr_methods = set(SERVER_REQUEST_ACCESSORS.keys())
    missing_sr = sr_methods_from_schema - accessor_sr_methods
    extra_sr = accessor_sr_methods - sr_methods_from_schema
    if missing_sr:
        validation_errors.append(
            f"SERVER_REQUEST_ACCESSORS missing {len(missing_sr)} methods "
            f"from schema: {sorted(missing_sr)}"
        )
    if extra_sr:
        validation_errors.append(
            f"SERVER_REQUEST_ACCESSORS has {len(extra_sr)} methods "
            f"not in schema: {sorted(extra_sr)}"
        )

    # Validate ClientRequest coverage
    schema_cr_methods = {m for m, _ in cr_methods}
    wrapper_cr_methods = set(CLIENT_REQUEST_WRAPPERS.keys())
    missing_cr = schema_cr_methods - wrapper_cr_methods
    extra_cr = wrapper_cr_methods - schema_cr_methods
    if missing_cr:
        validation_errors.append(
            f"CLIENT_REQUEST_WRAPPERS missing {len(missing_cr)} methods "
            f"from schema: {sorted(missing_cr)}"
        )
    if extra_cr:
        validation_errors.append(
            f"CLIENT_REQUEST_WRAPPERS has {len(extra_cr)} methods "
            f"not in schema: {sorted(extra_cr)}"
        )

    if validation_errors:
        print("VALIDATION ERRORS:", file=sys.stderr)
        for err in validation_errors:
            print(f"  - {err}", file=sys.stderr)
        sys.exit(1)

    print(
        f"Validated: {len(NOTIFICATION_ACCESSORS)} notifications, "
        f"{len(SERVER_REQUEST_ACCESSORS)} server requests, "
        f"{len(CLIENT_REQUEST_WRAPPERS)} client requests"
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
