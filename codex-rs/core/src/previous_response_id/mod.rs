//! Previous Response ID Support
//!
//! This module enables incremental conversation continuity for providers that support
//! the `previous_response_id` field (OpenAI Responses API).
//!
//! # Architecture
//!
//! ```text
//! User Input → build_turn_input() → Incremental or Full History
//!     ↓
//! ModelClient.stream(prompt, session) → Populate previous_response_id
//!     ↓
//! Adapter → Include previous_response_id in request
//!     ↓
//! On response → Store response_id in SessionState
//! ```
//!
//! # Key Components
//!
//! - [`input_builder::build_turn_input()`] - Decides incremental vs full input
//!
//! # Integration Points
//!
//! - `codex.rs::build_turn_input()` - Replaces direct history access with incremental logic
//! - `codex.rs::ResponseEvent::Completed` - Stores response_id + history_len after completion
//! - `codex.rs::PreviousResponseNotFound` - Error recovery with tracking clear
//! - `compact.rs::compact_task()` - Clears tracking after compaction
//! - `undo.rs::UndoTask::run()` - Clears tracking after undo
//! - `client.rs` - Detects `previous_response_not_found` API errors
//!
//! # Incremental Mode Behavior
//!
//! When all conditions are met:
//! 1. Adapter supports `previous_response_id` (via `supports_previous_response_id()`)
//! 2. SessionState has last_response tracking data (response_id + history_len)
//!
//! The system will:
//! - Send only items **after** the last assistant response (incremental)
//! - Append any pending input (tool outputs, user messages) to incremental history
//! - Include `previous_response_id` in the API request
//! - Reduce network payload significantly for long conversations
//!
//! Otherwise:
//! - Send complete conversation history (full mode)
//! - This is the default and safe fallback
//!
//! # Error Recovery
//!
//! When `previous_response_not_found` error occurs:
//! - Detected in `client.rs` and converted to `CodexErr::PreviousResponseNotFound`
//! - Session clears the invalid response_id from SessionState
//! - Automatic retry with full history (no incremental mode)
//! - Does not count against retry budget (logical error, not network error)
//!
//! # Example Flow
//!
//! ```text
//! Turn 1:
//!   Input: [UserMessage1]
//!   Mode: Full (no tracking data)
//!   Response: AssistantMessage1 (id=resp1)
//!   Store: SessionState tracking = (resp1, history_len=2)
//!
//! Turn 2:
//!   Input: [FunctionCall1, FunctionOutput1]  ← Incremental! (only new items)
//!   Mode: Incremental (has tracking data)
//!   Request: { previous_response_id: "resp1", input: [...new items only...] }
//!   Response: AssistantMessage2 (id=resp2)
//!   Store: SessionState tracking = (resp2, history_len=4)
//!
//! [COMPACT TRIGGERED]
//!   Action: Clear SessionState tracking data
//!
//! Turn 3:
//!   Input: [CompactedHistory, UserMessage2]
//!   Mode: Full (tracking cleared by compact)
//!   Response: AssistantMessage3 (id=resp3)
//!   Store: SessionState tracking = (resp3, history_len=2)
//! ```
//!
//! # Testing Strategy
//!
//! Unit tests in `input_builder.rs` cover:
//! - Incremental construction from response_id
//! - Full history fallback when no response_id
//! - History slicing logic
//! - Multiple assistant messages (uses last)
//!
//! Integration tests verify:
//! - End-to-end incremental flow with mock adapter
//! - Compact clears tracking data
//! - Model switch clears tracking data

pub mod input_builder;

pub use input_builder::build_turn_input;
