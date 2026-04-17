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
pub mod emit;
pub mod engine;
pub mod sdk_types;
mod session_state;
pub mod single_turn;
pub mod stream_accumulator;

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

// Re-export CoreEvent from coco-types for consumers of run_with_events().
// The old QueryEvent enum has been deleted per event-system-design.md Phase 0:
// QueryEngine now emits CoreEvent directly (3-layer Protocol/Stream/Tui dispatch).
pub use coco_types::AgentStreamEvent;
pub use coco_types::CoreEvent;
pub use coco_types::ServerNotification;
pub use stream_accumulator::StreamAccumulator;
