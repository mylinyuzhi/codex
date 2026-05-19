# coco-messages

Message **operations** crate: creation, normalization, filtering,
predicates, lookups, history persistence, cost tracking.

The Message-family **type definitions** themselves live in
`coco-types` (relocated alongside `ServerNotification` so the wire
enum can carry typed `Message` payloads). This crate re-exports them
at its crate root for backward compat with the established
`coco_messages::Message` import path used across the ops layer.

## TS Source
- `utils/messages.ts` — the largest utility file (~193K). All creation/filter/predicate functions.
- `utils/messages/mappers.ts`, `utils/messages/systemInit.ts` — mappers + system-init helpers
- `history.ts` — session history persistence
- `cost-tracker.ts` — token usage + cost tracking

## Key Types

### Re-exported from `coco-types::messages` (canonical home)

```rust
pub use coco_types::messages::*;
```

Surfaces the full Message family at `coco_messages::*`:

- **Message envelope**: `Message` (8 variants), `UserMessage`,
  `AssistantMessage`, `ToolResultMessage`, `AttachmentMessage`,
  `ProgressMessage`, `TombstoneMessage`, `ToolUseSummaryMessage`,
  `Visibility`, `MessageKind`, `MessageOrigin`, `StopReason`,
  `ApiError`, `PreservedSegment`, `PartialCompactDirection`.
- **System messages**: `SystemMessage` + 14 sub-variants
  (`SystemInformationalMessage`, `SystemApiErrorMessage`,
  `SystemCompactBoundaryMessage`, `SystemMicrocompactBoundaryMessage`,
  `SystemLocalCommandMessage`, `SystemPermissionRetryMessage`,
  `SystemBridgeStatusMessage`, `SystemMemorySavedMessage`,
  `SystemAwaySummaryMessage`, `SystemAgentsKilledMessage`,
  `SystemApiMetricsMessage`, `SystemStopHookSummaryMessage`,
  `SystemTurnDurationMessage`, `SystemScheduledTaskFireMessage`,
  `SystemUserInterruptionMessage`), `SystemMessageLevel`.
- **Attachment payloads**: `AttachmentBody`, `SilentPayload` + 10
  silent payload structs.
- **Tool result**: `ToolResult<T>` (carries `Vec<Message>`).
- **Hook result**: `HookResult` (embeds `Option<Message>`).
- **Persistence**: `SerializedMessage`, `TranscriptMessage`,
  `TranscriptEntry`.
- **LLM aliases (via `coco-llm-types`)**: `LlmMessage`, `LlmPrompt`,
  `UserContent`, `AssistantContent`, `ToolContent`, `TextContent`,
  `FileContent`, `ReasoningContent`, `ToolCallContent`,
  `ToolResultContent`.

### Owned here (operations only)

- **History**: `MessageHistory`
- **Cost**: `CostTracker`, `calculate_cost_usd`, `format_cost`, `get_model_pricing`
- **Creation**: `create_user_message`, `create_user_message_with_parts`,
  `create_assistant_message`, `create_assistant_error_message`,
  `create_cancellation_message`, `create_compact_boundary_message`,
  `create_error_tool_result`, `create_info_message`,
  `create_meta_message`, `create_permission_denied_message`,
  `create_progress_message`, `create_tool_result_message`,
  `create_user_interruption_system_message`
- **Normalize**: `normalize_messages_for_api`, `to_llm_prompt`,
  `ensure_user_first`, `merge_consecutive_user_messages`,
  `merge_consecutive_assistant_messages`, `strip_images_from_messages`,
  `strip_signature_blocks`
- **Lookups**: `MessageLookups`, `build_message_lookups`

## Module Layout

- `creation` — message constructors
- `normalize` — API-shape normalization (ensure user-first, merge consecutive, strip images/signatures)
- `filtering` — filter utilities
- `predicates` — is_* / has_* predicates
- `lookups` — O(1) index builders
- `wrapping` — message wrapping helpers
- `history` — persistence
- `cost` — token/cost tracking

Note: there is no `types/` submodule here anymore — type definitions
moved to `coco-types::messages` and are re-exported at the crate root.

## Vercel-AI Seam

`coco-messages` does **not** depend on `coco-inference`. DTO content
shapes reach this crate via `coco-types` (which depends on
`coco-llm-types`). Runtime types (`LanguageModel` trait, `ApiClient`,
etc.) are inference's domain and not needed for ops-layer work.

## Architecture

Internal messages embed an `LlmMessage` body directly — no twin
types, no conversion layer. `coco-llm-types` provides the
version-stripped `LlmMessage` alias so SDK upgrades stay scoped to
`common/llm-types/src/lib.rs` + `services/inference/src/lib.rs`.
