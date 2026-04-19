//! Keybinding list validator.
//!
//! TS: keybindings/ validator — flags parse errors, duplicate (context,
//! chord) pairs, and chords bound to different actions in the same
//! context (the latter is the real source of silent override bugs).

use std::collections::HashMap;

use crate::Keybinding;
use crate::parser;

/// A problem found in a keybinding list. Bubbled up for surfacing via
/// `doctor` or settings errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    /// The `key` field failed to parse.
    ParseError {
        index: usize,
        key: String,
        error: String,
    },
    /// Two bindings target the same (context, chord); the later one wins
    /// silently, which is almost always a bug.
    DuplicateBinding {
        context: Option<String>,
        key: String,
        first_action: String,
        later_action: String,
    },
}

/// Validate a keybinding list. Returns every problem found; empty vec
/// means clean.
pub fn validate(bindings: &[Keybinding]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut seen: HashMap<(Option<String>, String), (usize, String)> = HashMap::new();

    for (i, binding) in bindings.iter().enumerate() {
        let chord = match parser::parse_chord(&binding.key) {
            Ok(c) => c,
            Err(e) => {
                issues.push(ValidationIssue::ParseError {
                    index: i,
                    key: binding.key.clone(),
                    error: e,
                });
                continue;
            }
        };
        // Normalize the canonical form so `"Ctrl+A"` and `"ctrl+a"`
        // collide during duplicate detection.
        let canonical = canonical_chord(&chord);
        let key = (binding.context.clone(), canonical);
        if let Some((first_i, first_action)) = seen.get(&key) {
            if *first_action != binding.action {
                issues.push(ValidationIssue::DuplicateBinding {
                    context: binding.context.clone(),
                    key: binding.key.clone(),
                    first_action: first_action.clone(),
                    later_action: binding.action.clone(),
                });
            }
            let _ = first_i; // index preserved for future tooling
        } else {
            seen.insert(key, (i, binding.action.clone()));
        }
    }

    issues
}

/// Canonical string for duplicate detection. Modifiers in fixed order.
fn canonical_chord(chord: &crate::parser::KeyChord) -> String {
    chord
        .0
        .iter()
        .map(|c| {
            let mut parts: Vec<&str> = Vec::new();
            if c.ctrl {
                parts.push("ctrl");
            }
            if c.shift {
                parts.push("shift");
            }
            if c.alt {
                parts.push("alt");
            }
            if c.meta {
                parts.push("meta");
            }
            parts.push(c.key.as_str());
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[path = "validator.test.rs"]
mod tests;
