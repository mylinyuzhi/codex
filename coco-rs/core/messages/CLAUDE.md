# coco-messages

Message creation, normalization, filtering, predicates, lookups, history persistence, cost tracking — **plus** the Message-family type definitions themselves (relocated from `coco-types` so the foundation crate stays vercel-ai-free).

## TS Source
- `utils/messages.ts` — the largest utility file (~193K). All creation/filter/predicate functions.
- `utils/messages/mappers.ts`, `utils/messages/systemInit.ts` — mappers + system-init helpers
- `history.ts` — session history persistence
- `cost-tracker.ts` — token usage + cost tracking

## Key Types

### Owned here (relocated from `coco-types` for the seam)

Lives in the `types/` submodule, re-exported at crate root.

- **Message envelope**: `Message` (8 variants), `UserMessage`, `AssistantMessage`, `ToolResultMessage`, `AttachmentMessage`, `ProgressMessage`, `TombstoneMessage`, `ToolUseSummaryMessage`, `Visibility`, `MessageKind`, `MessageOrigin`, `StopReason`, `ApiError`, `PreservedSegment`, `PartialCompactDirection`.
- **System messages**: `SystemMessage` + 14 sub-variants (`SystemInformationalMessage`, `SystemApiErrorMessage`, `SystemCompactBoundaryMessage`, `SystemMicrocompactBoundaryMessage`, `SystemLocalCommandMessage`, `SystemPermissionRetryMessage`, `SystemBridgeStatusMessage`, `SystemMemorySavedMessage`, `SystemAwaySummaryMessage`, `SystemAgentsKilledMessage`, `SystemApiMetricsMessage`, `SystemStopHookSummaryMessage`, `SystemTurnDurationMessage`, `SystemScheduledTaskFireMessage`), `SystemMessageLevel`.
- **Attachment payloads**: `AttachmentBody`, `SilentPayload` + 10 silent payload structs (`HookCancelledPayload`, `HookErrorDuringExecutionPayload`, `HookNonBlockingErrorPayload`, `HookSystemMessagePayload`, `HookPermissionDecisionPayload`, `HookPermissionDecision`, `CommandPermissionsPayload`, `StructuredOutputPayload`, `DynamicSkillPayload`, `AlreadyReadFilePayload`, `EditedImageFilePayload`), `AttachmentEmitter`.
- **Tool result**: `ToolResult<T>` (carries `Vec<Message>` so it lives here).
- **Hook result**: `HookResult` (embeds `Option<Message>`).
- **Persistence**: `SerializedMessage` (session JSONL root), `TranscriptMessage` + `TranscriptEntry` (transcript archive).
- **LLM aliases (via `coco-inference`)**: `LlmMessage`, `LlmPrompt`, `UserContent`, `AssistantContent`, `ToolContent`, `TextContent`, `FileContent`, `ReasoningContent`, `ToolCallContent`, `ToolResultContent`.

### Functions

- **History**: `MessageHistory`
- **Cost**: `CostTracker`, `calculate_cost_usd`, `format_cost`, `get_model_pricing`
- **Creation**: `create_user_message`, `create_user_message_with_parts`, `create_assistant_message`, `create_assistant_error_message`, `create_cancellation_message`, `create_compact_boundary_message`, `create_error_tool_result`, `create_info_message`, `create_meta_message`, `create_permission_denied_message`, `create_progress_message`, `create_tool_result_message`
- **Normalize**: `normalize_messages_for_api`, `to_llm_prompt`, `ensure_user_first`, `merge_consecutive_user_messages`, `merge_consecutive_assistant_messages`, `strip_images_from_messages`, `strip_signature_blocks`
- **Lookups**: `MessageLookups`, `build_message_lookups`

## Module Layout

- `types/` — relocated Message-family definitions. Submodules: `aliases`, `message`, `attachment_body`, `attachment_emitter`, `serialized_message`, `hook_result`, `tool_result`, `transcript`. All exported at crate root.
- `creation` — message constructors
- `normalize` — API-shape normalization (ensure user-first, merge consecutive, strip images/signatures)
- `filtering` — filter utilities
- `predicates` — is_* / has_* predicates
- `lookups` — O(1) index builders
- `wrapping` — message wrapping helpers
- `history` — persistence
- `cost` — token/cost tracking

## Vercel-AI Seam

`coco-messages` reaches vercel-ai types through `coco-inference`'s version-agnostic re-exports — never `vercel_ai_provider::*` directly. The aliases module (`types/aliases.rs`) does:

```rust
pub use coco_inference::LanguageModelMessage as LlmMessage;
pub use coco_inference::AssistantContentPart as AssistantContent;
// ... etc.
```

Upgrading the underlying SDK (V4 → V5) only requires editing `services/inference/src/lib.rs`; this crate's content stays unchanged.

## Architecture

Internal messages embed an `LlmMessage` body directly — no twin types, no conversion layer. The seam at `coco-inference` keeps the version digit (`V4`) out of every consumer's path.
