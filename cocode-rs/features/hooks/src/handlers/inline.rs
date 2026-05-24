//! Inline handler type.
//!
//! Allows registering Rust closures as hook handlers. Inline handlers are not
//! serializable and can only be registered programmatically.

use crate::context::HookContext;
use crate::result::HookResult;

/// A function-based hook handler.
///
/// This type alias allows registering closures as hook handlers. The closure
/// receives a `HookContext` and returns a `HookResult`.
///
/// Inline handlers are not serializable and must be registered through the
/// `HookRegistry` API (not via JSON config).
pub type InlineHandler = Box<dyn Fn(&HookContext) -> HookResult + Send + Sync>;

#[cfg(test)]
#[path = "inline.test.rs"]
mod tests;
