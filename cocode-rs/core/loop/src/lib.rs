//! Agent loop driver for multi-turn conversations with LLM providers.

mod compaction;
mod driver;
mod error;
mod fallback;
mod result;
mod session_memory_agent;

pub use compaction::CompactConfig;
pub use compaction::ContextRestoration;
pub use compaction::FileRestoration;
pub use compaction::InvokedSkillRestoration;
pub use compaction::SessionMemorySummary;
pub use compaction::build_context_restoration;
pub use compaction::format_restoration_message;
pub use compaction::try_session_memory_compact;

// Micro-compact execution and threshold status
pub use compaction::CLEARED_CONTENT_MARKER;
pub use compaction::COMPACTABLE_TOOLS;
pub use compaction::CONTENT_PREVIEW_LENGTH;
pub use compaction::TaskInfo;
pub use compaction::TaskStatusRestoration;
pub use compaction::ThresholdStatus;
pub use compaction::build_compact_instructions;
pub use compaction::format_restoration_with_tasks;

// Summary formatting and context restoration
pub use compaction::build_token_breakdown;
pub use compaction::format_summary_with_transcript;
pub use compaction::wrap_hook_additional_context;

// File state rebuild and cleanup
pub use compaction::LRU_MAX_ENTRIES;
pub use compaction::LRU_MAX_SIZE_BYTES;
pub use compaction::build_file_read_state;
pub use compaction::is_internal_file;

// Re-export protocol types used in compaction
pub use compaction::CompactBoundaryMetadata;
pub use compaction::CompactTelemetry;
pub use compaction::CompactTrigger;
pub use compaction::HookAdditionalContext;
pub use compaction::MemoryAttachment;
pub use compaction::PersistedToolResult;
pub use compaction::TokenBreakdown;

pub use driver::AgentLoop;
pub use driver::AgentLoopBuilder;
pub use error::AgentLoopError;
pub use fallback::FallbackAttempt;
pub use fallback::FallbackConfig;
pub use fallback::FallbackState;
pub use result::LoopResult;
pub use result::StopReason;
pub use session_memory_agent::ExtractionOutcome;
pub use session_memory_agent::ExtractionResult;
pub use session_memory_agent::SessionMemoryExtractionAgent;

// Re-export LoopConfig and AgentStatus from cocode-protocol
pub use cocode_protocol::AgentStatus;
pub use cocode_protocol::LoopConfig;
