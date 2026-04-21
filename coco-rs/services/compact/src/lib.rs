//! Context compaction: full (LLM summarize), micro (tool result clearing),
//! API-level clearing, reactive with circuit breaker, and auto-trigger.
//!
//! TS: services/compact/ (compact.ts, microCompact.ts, autoCompact.ts, apiMicrocompact.ts)

pub mod api_compact;
pub mod auto_trigger;
pub mod compact;
pub mod grouping;
pub mod micro;
pub mod micro_advanced;
pub mod observer;
pub mod post_compact_files;
pub mod post_compact_plan;
pub mod prompt;
pub mod reactive;
pub mod session_memory;
pub mod tokens;
pub mod types;

// ── Re-exports for ergonomic use ────────────────────────────────────

pub use api_compact::clear_thinking;
pub use api_compact::clear_tool_uses;
pub use auto_trigger::TimeBasedMcConfig;
pub use auto_trigger::auto_compact_threshold;
pub use auto_trigger::calculate_token_warning_state;
pub use auto_trigger::effective_context_window;
pub use auto_trigger::should_auto_compact;
pub use compact::CompactConfig;
pub use compact::compact_conversation;
pub use compact::strip_images_from_messages;
pub use compact::strip_reinjected_attachments;
pub use compact::truncate_head_for_ptl_retry;
pub use micro::micro_compact;
pub use micro_advanced::MicroCompactBudgetConfig;
pub use micro_advanced::clear_file_unchanged_stubs;
pub use micro_advanced::compact_thinking_blocks;
pub use micro_advanced::micro_compact_with_budget;
pub use observer::CompactionObserver;
pub use observer::CompactionObserverRegistry;
pub use post_compact_files::create_post_compact_file_attachments;
pub use post_compact_plan::create_plan_attachment_from_owned;
pub use post_compact_plan::create_plan_attachment_if_needed;
pub use prompt::format_compact_summary;
pub use prompt::get_compact_prompt;
pub use prompt::get_compact_user_summary_message;
pub use prompt::get_partial_compact_prompt;
pub use reactive::ReactiveCompactConfig;
pub use reactive::ReactiveCompactState;
pub use session_memory::SessionMemoryCompactConfig;
pub use session_memory::compact_session_memory;
pub use session_memory::has_text_blocks;
pub use session_memory::merge_similar_memories;
pub use session_memory::select_memories_for_compaction;
pub use tokens::estimate_message_tokens;
pub use tokens::estimate_tokens;
pub use tokens::estimate_tokens_conservative;
pub use types::CompactError;
pub use types::CompactResult;
pub use types::ContextEditStrategy;
pub use types::MicrocompactResult;
pub use types::TokenWarningState;
