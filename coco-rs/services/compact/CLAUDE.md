# coco-compact

Context compaction strategies: full LLM-summarized, micro (tool-result clearing), API-level, reactive (prompt-too-long), session-memory, auto-trigger.

## TS Source
- `services/compact/compact.ts` — full LLM-summarized compaction (largest)
- `services/compact/microCompact.ts` — clear old tool results
- `services/compact/apiMicrocompact.ts` — API-level thinking/tool clearing
- `services/compact/autoCompact.ts` — threshold-based auto-trigger
- `services/compact/sessionMemoryCompact.ts` — session memory compaction
- `services/compact/grouping.ts` — message grouping for compaction
- `services/compact/postCompactCleanup.ts` — file-attachment re-injection post-compact
- `services/compact/prompt.ts` — summary prompt templates
- `services/compact/timeBasedMCConfig.ts` — time-based MC config

**Intentionally NOT ported**: TS `HISTORY_SNIP` (cache-aware pre-microcompact snipping) and `CONTEXT_COLLAPSE` (`collapseReadSearch`, `collapseBackgroundBashNotifications`, `collapseHookSummaries`, `collapseTeammateShutdowns`) — these are Anthropic prompt-cache + protected-tail optimizations. If a provider needs cache-aware compaction, add it in the `vercel-ai-<provider>` crate.

## Key Types

- `CompactResult`, `MicrocompactResult`, `ContextEditStrategy`, `TokenWarningState`, `CompactError`
- `CompactConfig`, `compact_conversation`, `truncate_head_for_ptl_retry`
- `micro_compact`, `MicroCompactBudgetConfig`, `micro_compact_with_budget`, `compact_thinking_blocks`, `clear_file_unchanged_stubs`
- `clear_thinking`, `clear_tool_uses` — API-level clearing
- `ReactiveCompactConfig`, `ReactiveCompactState` — circuit breaker for prompt_too_long retries
- `TimeBasedMcConfig`, `should_auto_compact`, `auto_compact_threshold`, `effective_context_window`, `calculate_token_warning_state`
- `SessionMemoryCompactConfig`, `compact_session_memory`, `select_memories_for_compaction`, `merge_similar_memories`
- `CompactionObserver`, `CompactionObserverRegistry` — lifecycle hooks
- `create_post_compact_file_attachments` — post-compact file re-injection
- `get_compact_prompt`, `get_partial_compact_prompt`, `format_compact_summary`, `get_compact_user_summary_message`
- `strip_images_from_messages`, `strip_reinjected_attachments`, `has_text_blocks`
- `estimate_tokens`, `estimate_tokens_conservative`, `estimate_message_tokens`
