//! Previous Response ID Support (Stateless Filtering Architecture)
//!
//! This module enables incremental conversation continuity for providers that support
//! the `previous_response_id` field (OpenAI Responses API).
//!
//! # Architecture (Stateless Filtering)
//!
//! ```text
//! User Input → build_turn_input() → Filter history by item type
//!     ↓
//! Identify LLM-generated items (have `id` field) → Filter them out
//!     ↓
//! Return only user inputs (tool outputs, user messages)
//!     ↓
//! ModelClient.stream(prompt, session) → Include previous_response_id
//!     ↓
//! Server already has LLM outputs → Only needs new user inputs
//! ```
//!
//! # Key Innovation: Minimal State Tracking
//!
//! Unlike traditional approaches that track both history length AND response IDs,
//! this implementation only tracks `last_response_id` and uses **stateless filtering** for history:
//!
//! - **State tracked**: Only `last_response_id: Option<String>` (no history_len)
//! - **Filtering logic**: Type-based classification (stateless)
//! - **LLM-generated items** (server already has them): Have `id: Option<String>` field
//!   - `Message { role: "assistant" }`, `Reasoning`, `FunctionCall`, `CustomToolCall`, etc.
//! - **User input items** (server needs them): Have only `call_id` or no ID field
//!   - `FunctionCallOutput`, `CustomToolCallOutput`, `Message { role: "user" }`, etc.
//!
//! # Key Components
//!
//! - [`input_builder::build_turn_input()`] - Decides incremental vs full input
//! - [`input_builder::is_llm_generated()`] - Type-based classification (private)
//! - [`input_builder::build_incremental_input_filtered()`] - Stateless filtering logic
//!
//! # Integration Points
//!
//! - `codex.rs::build_turn_input()` - Calls filtering logic to build incremental input
//! - `codex.rs` line ~1848 - Sets `last_response_id` after response completion
//! - `session.rs::replace_history_and_clear_tracking()` - Clears tracking on compact/undo
//! - `codex.rs` line ~2000 - Clears tracking on `PreviousResponseNotFound` error
//!
//! **Simplified compared to old architecture:**
//! - ❌ No `history_len` tracking (removed)
//! - ❌ No `set_last_response_from_current_history()` (removed)
//! - ✅ Only minimal `last_response_id` state (kept for HTTP header)
//!
//! # Incremental Mode Behavior
//!
//! When adapter supports `previous_response_id`:
//! 1. Find last LLM-generated item in history (marks end of last response)
//! 2. Return only USER INPUT items after that point
//! 3. Filter out any remaining LLM items (defensive, should not happen)
//! 4. Append pending input (tool outputs, user messages during execution)
//!
//! **Benefits:**
//! - **Minimal state**: Only tracks `response_id`, not `history_len`
//! - **Self-correcting filtering**: Always computes correct items based on types
//! - **Simpler**: ~50 lines instead of ~500, 2 files instead of 15
//! - **Robust lifecycle**: Compact/undo auto-clear via `replace_history_and_clear_tracking()`
//!
//! # Error Recovery
//!
//! When `previous_response_not_found` error occurs:
//! - Detected in `client.rs` and converted to `CodexErr::PreviousResponseNotFound`
//! - Handler calls `clear_last_response()` to invalidate stale ID
//! - Automatic retry with full history (filtering detects no previous_response_id)
//! - Does not count against retry budget (logical error, not network error)
//!
//! # Example Flow (Stateless Filtering)
//!
//! ```text
//! Turn 1:
//!   History: [UserMessage1]
//!   Filter: No LLM outputs yet → Full history
//!   Response: AssistantMessage1, FunctionCall1
//!   History: [UserMessage1, AssistantMessage1, FunctionCall1]
//!
//! Turn 2:
//!   Tool execution adds: FunctionCallOutput1
//!   History: [UserMsg1, AssistMsg1, FuncCall1, FuncOutput1]
//!   Filter:
//!     - Last LLM item: FuncCall1 (index 2)
//!     - Items after: [FuncOutput1]
//!     - Filter LLM: [FuncOutput1] (no LLM items)
//!   User adds: UserMessage2
//!   Incremental input: [FuncOutput1, UserMsg2]
//!   Request: { previous_response_id: "resp1", input: [FuncOutput1, UserMsg2] }
//!
//! [COMPACT TRIGGERED]
//!   replace_history_and_clear_tracking() called
//!   History replaced with compacted summary
//!   last_response_id cleared to None
//!   Filter: Last LLM item = compacted AssistantMessage
//!   Next turn: Sends full history (no previous_response_id)
//!
//! Turn 3:
//!   History: [CompactedAssistantMsg, UserMessage3]
//!   Filter: Last LLM item at index 0 → [UserMsg3]
//!   Works correctly without any manual tracking management
//! ```
//!
//! # Testing Strategy
//!
//! Unit tests in `input_builder.rs` cover:
//! - `is_llm_generated()` correctly identifies item types
//! - Filtering returns only user inputs after last LLM output
//! - Handles no LLM outputs (first turn)
//! - Handles multiple tool outputs
//!
//! Integration tests in `incremental_input.rs` verify:
//! - Tool outputs delivered correctly (not lost)
//! - Multiple parallel tool calls work
//! - First turn sends full history
//! - Error recovery clears stale response_id and retries
//! - Compact/undo clear response_id correctly
//! - Empty input validation falls back to full history
//! - Pending user messages appended correctly

pub mod input_builder;

pub use input_builder::build_turn_input;
