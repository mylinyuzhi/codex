//! Session-memory extraction pipeline.
//!
//! TS source: `services/SessionMemory/sessionMemory.ts` (forked-agent
//! extraction loop), `services/SessionMemory/sessionMemoryUtils.ts`
//! (path resolution + lastSummarizedMessageId), `services/SessionMemory/prompts.ts`
//! (extraction prompt template).
//!
//! Responsibilities:
//!   - Resolve the on-disk session-memory file path
//!     (`<config_home>/sessions/<session_id>/session-memory/summary.md`).
//!   - Read existing memory content into a cached string.
//!   - Run the post-sampling extraction trigger
//!     (`coco_compact::should_extract_memory`) and, when fired, call a
//!     forked-agent summarizer to produce a fresh memory body.
//!   - Atomically write the new body back to disk.
//!   - Track `lastSummarizedMessageId` in-memory so the SM-first
//!     compact path can detect re-extraction boundaries.
//!
//! This crate is **caller-driven**: hosts (CLI / SDK runners) hold a
//! `SessionMemoryService` and invoke `maybe_extract` after each
//! assistant turn. The SM-first compact path in `app/query` reads the
//! latest text via `current_text()` between turns.

pub mod path;
pub mod prompts;
pub mod service;

pub use path::session_memory_path;
pub use prompts::default_extraction_prompt;
pub use service::ExtractionDecision;
pub use service::ExtractionOutcome;
pub use service::SessionMemoryService;
pub use service::SummarizerFn;
pub use service::count_tool_calls_in_last_assistant_turn;
