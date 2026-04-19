# coco-messages

Message creation, normalization, filtering, predicates, lookups, history persistence, cost tracking.

## TS Source
- `utils/messages.ts` — the largest utility file (~193K). All creation/filter/predicate functions.
- `utils/messages/mappers.ts`, `utils/messages/systemInit.ts` — mappers + system-init helpers
- `history.ts` — session history persistence
- `cost-tracker.ts` — token usage + cost tracking

## Key Types

- **History**: `MessageHistory`
- **Cost**: `CostTracker`, `calculate_cost_usd`, `format_cost`, `get_model_pricing`
- **Creation**: `create_user_message`, `create_user_message_with_parts`, `create_assistant_message`, `create_assistant_error_message`, `create_cancellation_message`, `create_compact_boundary_message`, `create_error_tool_result`, `create_info_message`, `create_meta_message`, `create_permission_denied_message`, `create_progress_message`, `create_tool_result_message`
- **Normalize**: `normalize_messages_for_api`, `to_llm_prompt`, `ensure_user_first`, `merge_consecutive_user_messages`, `merge_consecutive_assistant_messages`, `strip_images_from_messages`, `strip_signature_blocks`
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

## Architecture

Internal messages wrap vercel-ai `LlmMessage` directly (mirroring TS's `@anthropic-ai/sdk` nested pattern). Normalization filters + reorders but does not transform types — the API call takes `.message` directly. See main CLAUDE.md "Message Model" for the TS-vs-Rust discussion.
