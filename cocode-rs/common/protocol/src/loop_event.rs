//! Re-exports shared event supporting types.
//!
//! This module previously defined `LoopEvent`. All supporting types have been
//! moved to [`crate::event_types`] and are re-exported here for backward
//! compatibility.

// Re-export all supporting types from event_types for backward compatibility.
// External crates that do `use cocode_protocol::loop_event::TokenUsage` continue to work.
pub use crate::event_types::*;
