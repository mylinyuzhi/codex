# coco-types

Foundation types shared across all crates. **Zero LLM dependencies** — provider-specific types live in `coco-messages` (which reaches vercel-ai through the `coco-inference` seam). This crate stays provider-agnostic.

## TS Source
- `types/` — `command.ts`, `hooks.ts`, `ids.ts`, `logs.ts`, `permissions.ts`, `plugin.ts`, `textInputTypes.ts`, `generated/` (build-time message types)
- `Tool.ts` — foundational tool identity (`ToolInputSchema`, `ToolName`, `ToolId`, `ToolProgress`). `ToolResult<T>` (which carries `Vec<Message>`) lives in `coco-messages`.
- `Task.ts` — task lifecycle (`TaskType`, `TaskStatus`, `TaskStateBase`)

## Key Types

Tool / Agent identity: `ToolName` (41 builtin variants, Copy), `ToolId` (Builtin/Mcp/Custom, flat-string serde), `SubagentType` (7 builtin variants), `AgentTypeId`, `ToolInputSchema`, `ToolProgress`.

Permission: `PermissionMode` (camelCase wire), `PermissionBehavior`, `PermissionRule`, `PermissionRuleSource`, `PermissionDecision`, `PermissionDecisionReason`, `ToolPermissionContext`.

Hook / Task / Command: `HookEventType` (32 variants, `#[non_exhaustive]`), `HookOutcome`, `HookScope`, `TaskType`, `TaskStatus`, `TaskStateBase`, `CommandBase`, `CommandType`, `CommandSource`. (`HookResult` lives in `coco-messages` because it carries `Option<Message>`.)

Provider / Model: `ProviderApi`, `ModelRole`, `ModelSpec`, `Capability`, `CapabilitySet`, `ApplyPatchToolType`, `WireApi`.

Thinking / Token / ID / Sandbox: `ThinkingLevel { effort, budget_tokens, options }`, `ReasoningEffort` (7 variants: `Disable`, `Auto`, `Minimal`, `Low`, `Medium`, `High`, `XHigh` — `Disable` is explicit-off, `Auto` defers to provider default, the rest are explicit numeric levels gated by `is_explicit_level()`), `TokenUsage`, `ModelUsage`, `SessionId`, `AgentId`, `TaskId`, `SandboxMode`.

Event envelope (owned here — see `event-system-design.md`): `CoreEvent` (3-layer), `ServerNotification` (66 variants) + `NotificationMethod` (typed wire-method enum), `AgentStreamEvent`, `TuiOnlyEvent`, `ThreadItem`, plus 50+ event param structs.

Wire protocol: `ClientRequest` + `ClientRequestMethod` (30 variants), `ServerRequest` + `ServerRequestMethod` (5 variants), `JsonRpcMessage` family, `RequestId`, `error_codes`.

Attachment taxonomy: `AttachmentKind` (60 variants), `AttachmentEvent`, `Coverage`, `coverage_of`. Per-variant payloads (`AttachmentBody`, `SilentPayload`, `HookCancelledPayload`, …) live in `coco-messages` because the `Api` body embeds an `LlmMessage`.

App-state: `ToolAppState`, `AppStatePatch`, `AppStateReadHandle` (typed cross-turn state).

Extended (ported TS extensions): `AgentColorEntry`, `AttributionSnapshotEntry`, `CommandResultDisplay`, `PermissionExplanation`, `PromptRequest`, `RiskLevel`, `SessionMode`, `SummaryEntry`, etc. (`TranscriptMessage` and `TranscriptEntry` moved to `coco-messages` because they embed `Message`.)

`CompactTrigger` lives here (rather than with the rest of the message family) because `event::CompactionPhaseParams` references it; keeping it in `coco-types` avoids a back-edge from the event layer to `coco-messages`.

### Wire-tagged-enum macro

`ServerNotification`, `ClientRequest`, and `ServerRequest` are emitted via the `wire_tagged_enum!` macro (`src/wire_tagged.rs`). From a single `"wire-string" => Variant` table the macro derives:

1. The tagged union (`#[serde(tag = "method", content = "params")]`) with per-variant `#[serde(rename = "wire/method")]`.
2. A companion `FooMethod` Copy enum with `serde` + `strum::Display` + `strum::IntoStaticStr` + `JsonSchema`.
3. A `pub const fn method(&self) -> FooMethod` accessor on the tagged union.

The same `$wire` literal drives `#[serde(rename)]` **and** `#[strum(serialize)]`, so the wire string cannot drift across accessors, schema, or cross-language codegens that consume `notification_method.json` / `client_request_method.json` / `server_request_method.json`.

## Vercel-AI Seam

`coco-types` no longer depends on `vercel-ai-provider`. Anything that needs LLM types (`LlmMessage`, `UserContent`, `ToolResultContent`, …) imports them from `coco-messages` (which uses `coco-inference` re-exports under the hood). The seam crate `services/inference` is the single workspace owner of `vercel-ai-provider` — guarded by `scripts/check-vercel-ai-seam.sh`.

## Conventions

- `ToolId` / `AgentTypeId` serialize as flat strings via `Display` / `FromStr` (not tagged JSON). `"Read"` / `"mcp__slack__send"` / `"my_plugin_tool"`.
- `PermissionMode` wire format is camelCase (matches TS `PermissionModeSchema`). Snake-case aliases accepted on deserialize for legacy transcripts.
- `side_query` module contains data types for the async `SideQuery` trait (trait itself lives in `coco-tool-runtime`).
