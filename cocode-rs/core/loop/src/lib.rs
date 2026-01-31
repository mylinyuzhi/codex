//! Agent loop driver for multi-turn conversations with LLM providers.

mod compaction;
mod driver;
mod fallback;
mod result;
mod session_memory_agent;

pub use compaction::{
    CompactConfig, CompactionConfig, CompactionResult, CompactionTier, ContextRestoration,
    FileRestoration, InvokedSkillRestoration, SessionMemoryConfig, SessionMemorySummary,
    build_context_restoration, format_restoration_message, micro_compact_candidates,
    should_compact, try_session_memory_compact,
};

// Phase 2: Micro-compact execution and threshold status
pub use compaction::{
    CLEARED_CONTENT_MARKER, COMPACTABLE_TOOLS, CONTENT_PREVIEW_LENGTH, MicroCompactResult,
    TaskInfo, TaskStatusRestoration, ThresholdStatus, ToolResultCandidate,
    build_compact_instructions, execute_micro_compact, format_restoration_with_tasks,
};

// Phase 3: Summary formatting and context restoration
pub use compaction::{
    build_token_breakdown, create_compact_boundary_message, create_invoked_skills_attachment,
    format_summary_with_transcript, wrap_hook_additional_context,
};

// Re-export protocol types used in compaction
pub use compaction::{
    CompactBoundaryMetadata, CompactTelemetry, CompactTrigger, HookAdditionalContext,
    MemoryAttachment, PersistedToolResult, TokenBreakdown,
};

// Re-export backwards-compatible constant names
pub use compaction::{
    CONTEXT_RESTORATION_BUDGET, CONTEXT_RESTORATION_MAX_FILES, MIN_MICRO_COMPACT_SAVINGS,
    RECENT_TOOL_RESULTS_TO_KEEP,
};
pub use driver::{AgentLoop, AgentLoopBuilder};
pub use fallback::{FallbackAttempt, FallbackConfig, FallbackState};
pub use result::{LoopResult, StopReason};
pub use session_memory_agent::{ExtractionResult, SessionMemoryExtractionAgent};

// Re-export LoopConfig from cocode-protocol
pub use cocode_protocol::LoopConfig;
