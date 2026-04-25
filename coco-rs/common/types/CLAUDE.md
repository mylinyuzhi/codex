# coco-types

Foundation types shared across all crates. Depends only on `vercel-ai-provider` (L0) for LLM content types.

## TS Source
- `types/` — `command.ts`, `hooks.ts`, `ids.ts`, `logs.ts`, `permissions.ts`, `plugin.ts`, `textInputTypes.ts`, `generated/` (build-time message types)
- `Tool.ts` — tool trait/schema (`ToolInputSchema`, `ToolResult<T>`, `ToolProgress`)
- `Task.ts` — task lifecycle (`TaskType`, `TaskStatus`, `TaskStateBase`)

## Key Types

Message layer: `Message`, `UserMessage`, `AssistantMessage`, `SystemMessage` (14 sub-variants), `AttachmentMessage`, `ToolResultMessage`, `ProgressMessage`, `TombstoneMessage`, `ToolUseSummaryMessage`, `StopReason`, `MessageOrigin`.

Tool / Agent identity: `ToolName` (41 builtin variants, Copy), `ToolId` (Builtin/Mcp/Custom, flat-string serde), `SubagentType` (7 builtin variants), `AgentTypeId`, `ToolInputSchema`, `ToolResult<T>`, `ToolProgress`.

Permission: `PermissionMode` (camelCase wire), `PermissionBehavior`, `PermissionRule`, `PermissionRuleSource`, `PermissionDecision`, `PermissionDecisionReason`, `ToolPermissionContext`.

Hook / Task / Command: `HookEventType` (27 variants, `#[non_exhaustive]`), `HookOutcome`, `HookResult`, `TaskType`, `TaskStatus`, `TaskStateBase`, `CommandBase`, `CommandType`, `CommandSource`.

Provider / Model: `ProviderApi`, `ModelRole`, `ModelSpec`, `Capability`, `CapabilitySet`, `ApplyPatchToolType`, `WireApi`.

Thinking / Token / ID / Sandbox: `ThinkingLevel { effort, budget_tokens, options }`, `ReasoningEffort` (6 levels), `TokenUsage`, `ModelUsage`, `SessionId`, `AgentId`, `TaskId`, `SandboxMode`.

Event envelope (owned here — see `event-system-design.md`): `CoreEvent` (3-layer), `ServerNotification` (66 variants) + `NotificationMethod` (typed wire-method enum), `AgentStreamEvent`, `TuiOnlyEvent`, `ThreadItem`, plus 50+ event param structs.

Wire protocol: `ClientRequest` + `ClientRequestMethod` (30 variants), `ServerRequest` + `ServerRequestMethod` (5 variants), `JsonRpcMessage` family, `RequestId`, `error_codes`.

### Wire-tagged-enum macro

`ServerNotification`, `ClientRequest`, and `ServerRequest` are emitted via the `wire_tagged_enum!` macro (`src/wire_tagged.rs`). From a single `"wire-string" => Variant` table the macro derives:

1. The tagged union (`#[serde(tag = "method", content = "params")]`) with per-variant `#[serde(rename = "wire/method")]`.
2. A companion `FooMethod` Copy enum with `serde` + `strum::Display` + `strum::IntoStaticStr` + `JsonSchema`.
3. A `pub const fn method(&self) -> FooMethod` accessor on the tagged union.

The same `$wire` literal drives `#[serde(rename)]` **and** `#[strum(serialize)]`, so the wire string cannot drift across accessors, schema, or cross-language codegens that consume `notification_method.json` / `client_request_method.json` / `server_request_method.json`.

App-state: `ToolAppState`, `AppStatePatch`, `AppStateReadHandle` (typed cross-turn state, formerly `serde_json::Value`).

Extended (ported TS extensions): `AgentColorEntry`, `AttributionSnapshotEntry`, `CommandResultDisplay`, `PermissionExplanation`, `PromptRequest`, `RiskLevel`, `SessionMode`, `TranscriptEntry`, etc.

## Version Isolation

Re-exports vercel-ai v4 types under version-agnostic aliases — callers must use these, never `vercel_ai_provider::*` directly:

| Alias | Source |
|-------|--------|
| `LlmMessage` | `LanguageModelV4Message` |
| `LlmPrompt` | `LanguageModelV4Prompt` |
| `UserContent` / `AssistantContent` / `ToolContent` | `*ContentPart` |
| `TextContent` / `FileContent` / `ReasoningContent` / `ToolCallContent` / `ToolResultContent` | `TextPart` / `FilePart` / `ReasoningPart` / `ToolCallPart` / `ToolResultPart` |

Upgrading vercel-ai only requires editing these re-exports in `lib.rs`.

## Conventions

- `ToolId` / `AgentTypeId` serialize as flat strings via `Display` / `FromStr` (not tagged JSON). `"Read"` / `"mcp__slack__send"` / `"my_plugin_tool"`.
- `PermissionMode` wire format is camelCase (matches TS `PermissionModeSchema`). Snake-case aliases accepted on deserialize for legacy transcripts.
- `side_query` module contains data types for the async `SideQuery` trait (trait itself lives in `coco-tool-runtime`).
