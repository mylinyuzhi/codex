//! Hook event types.
//!
//! Re-exports [`cocode_protocol::HookEventType`] as the single canonical definition.

pub use cocode_protocol::HookEventType;

#[cfg(test)]
#[path = "event.test.rs"]
mod tests;
