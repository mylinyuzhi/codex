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

Event envelope (owned here — see `event-system-design.md`): `CoreEvent` (3-layer), `ServerNotification` (52 variants), `AgentStreamEvent`, `TuiOnlyEvent`, `ThreadItem`, plus 50+ event param structs.

Wire protocol: `ClientRequest`, `ServerRequest`, `JsonRpcMessage` family, `RequestId`, `error_codes`.

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
- `side_query` module contains data types for the async `SideQuery` trait (trait itself lives in `coco-tool`).
