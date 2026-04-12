//! Query engine — the multi-turn agent loop.
//!
//! TS: QueryEngine.ts (46.6K) + query.ts (68.7K)
//!
//! The core cycle:
//! 1. Build system prompt (context)
//! 2. Normalize messages for API
//! 3. Call LLM via inference
//! 4. Parse response, extract tool calls
//! 5. Execute tools (via StreamingToolExecutor batch partitioning)
//! 6. Check stop conditions (no tool calls, max turns, budget)
//! 7. Auto-compact if needed
//! 8. Drain command queue + inject attachments
//! 9. Loop back to 1 if tool calls, else done

pub mod agent_adapter;
pub mod budget;
pub mod command_queue;
pub mod engine;
pub mod sdk_types;
pub mod single_turn;

pub use budget::BudgetDecision;
pub use budget::BudgetTracker;
pub use command_queue::CommandQueue;
pub use command_queue::Inbox;
pub use command_queue::InboxMessage;
pub use command_queue::QueryGuard;
pub use command_queue::QueryGuardStatus;
pub use command_queue::QueuePriority;
pub use command_queue::QueuedCommand;
pub use engine::ContinueReason;
pub use engine::QueryEngine;
pub use engine::QueryEngineConfig;
pub use engine::QueryResult;

/// Events emitted during query execution for progress tracking.
///
/// TS: LoopEvent variants in protocol/events + QueryEngine yield types.
#[derive(Debug, Clone)]
pub enum QueryEvent {
    /// A new turn has started.
    TurnStarted { turn: i32 },
    /// LLM returned text content (streaming delta).
    TextDelta { text: String },
    /// Reasoning/thinking content from the model.
    ReasoningDelta { text: String },
    /// A tool call is about to execute.
    ToolUseStart {
        tool_use_id: String,
        tool_name: String,
    },
    /// A tool call finished.
    ToolUseEnd {
        tool_use_id: String,
        tool_name: String,
        is_error: bool,
        duration_ms: i64,
    },
    /// The turn completed.
    TurnCompleted { turn: i32, has_tool_calls: bool },
    /// Auto-compaction was triggered.
    CompactionTriggered,
    /// Budget nudge warning.
    BudgetNudge { message: String },
    /// Model streaming started.
    StreamRequestStart { turn: i32, model: String },
    /// An error occurred but was recovered from.
    ErrorRecovery {
        reason: ContinueReason,
        message: String,
    },
    /// Queued commands were drained and injected.
    CommandsDrained { count: i32 },
    /// Inbox messages were consumed.
    InboxConsumed { count: i32 },
    /// Query is stopping.
    QueryStopping { reason: String },
}
