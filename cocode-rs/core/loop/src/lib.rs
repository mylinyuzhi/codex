//! Agent loop driver for multi-turn conversations with LLM providers.

mod compaction;
mod driver;
mod fallback;
mod result;

pub use compaction::{
    CompactConfig, CompactionConfig, CompactionResult, CompactionTier, ContextRestoration,
    FileRestoration, SessionMemoryConfig, SessionMemorySummary, build_context_restoration,
    format_restoration_message, micro_compact_candidates, should_compact,
    try_session_memory_compact,
};

// Phase 2: Micro-compact execution and threshold status
pub use compaction::{
    CLEARED_CONTENT_MARKER, COMPACTABLE_TOOLS, CONTENT_PREVIEW_LENGTH, MicroCompactResult,
    TaskInfo, TaskStatusRestoration, ThresholdStatus, ToolResultCandidate,
    build_compact_instructions, execute_micro_compact, format_restoration_with_tasks,
};

// Re-export backwards-compatible constant names
pub use compaction::{
    CONTEXT_RESTORATION_BUDGET, CONTEXT_RESTORATION_MAX_FILES, MIN_MICRO_COMPACT_SAVINGS,
    RECENT_TOOL_RESULTS_TO_KEEP,
};
pub use driver::{AgentLoop, AgentLoopBuilder};
pub use fallback::{FallbackAttempt, FallbackConfig, FallbackState};
pub use result::{LoopResult, StopReason};

// Re-export LoopConfig from cocode-protocol
pub use cocode_protocol::LoopConfig;
