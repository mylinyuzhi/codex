//! Binding merge strategy.
//!
//! Merges default bindings with user overrides using additive
//! last-match-wins semantics: user bindings are appended after defaults,
//! so the resolver's last-match-wins naturally gives priority to user
//! bindings.

use crate::resolver::Binding;

/// Merge default bindings with user-provided overrides.
///
/// Returns a combined binding table where user bindings appear after
/// defaults. The resolver's last-match-wins semantics ensures user
/// bindings shadow defaults with the same key+context.
pub fn merge_bindings(defaults: Vec<Binding>, user: Vec<Binding>) -> Vec<Binding> {
    let mut merged = defaults;
    merged.extend(user);
    merged
}

#[cfg(test)]
#[path = "merge.test.rs"]
mod tests;
