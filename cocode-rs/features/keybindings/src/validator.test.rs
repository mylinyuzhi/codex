use std::collections::BTreeMap;

use crate::config::ContextBindings;
use crate::config::KeybindingsFile;

use super::*;

type BindingEntry<'a> = (&'a str, Option<&'a str>);
type ContextBlock<'a> = (&'a str, Vec<BindingEntry<'a>>);

fn make_file(blocks: Vec<ContextBlock<'_>>) -> KeybindingsFile {
    KeybindingsFile {
        bindings: blocks
            .into_iter()
            .map(|(ctx, entries)| ContextBindings {
                context: ctx.to_string(),
                bindings: entries
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.map(ToString::to_string)))
                    .collect(),
            })
            .collect(),
    }
}

#[test]
fn test_valid_file_no_warnings() {
    let file = make_file(vec![(
        "Chat",
        vec![("ctrl+t", Some("ext:cycleThinkingLevel"))],
    )]);
    let warnings = validate_file(&file);
    assert!(warnings.is_empty());
}

#[test]
fn test_invalid_context() {
    let file = make_file(vec![(
        "NonExistent",
        vec![("ctrl+t", Some("app:interrupt"))],
    )]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidContext { .. }))
    );
}

#[test]
fn test_invalid_action() {
    let file = make_file(vec![("Chat", vec![("ctrl+t", Some("bad:action"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidAction { .. }))
    );
}

#[test]
fn test_invalid_keystroke() {
    let file = make_file(vec![("Chat", vec![("ctrl++", Some("app:interrupt"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidKeystroke { .. }))
    );
}

#[test]
fn test_duplicate_binding() {
    let file = KeybindingsFile {
        bindings: vec![ContextBindings {
            context: "Chat".to_string(),
            bindings: {
                let mut m = BTreeMap::new();
                m.insert("ctrl+t".to_string(), Some("app:interrupt".to_string()));
                m.insert("Ctrl+T".to_string(), Some("app:exit".to_string()));
                m
            },
        }],
    };
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::DuplicateBinding { .. }))
    );
}

#[test]
fn test_reserved_key_warning() {
    let file = make_file(vec![("Chat", vec![("ctrl+c", Some("chat:cancel"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::ReservedKey { .. }))
    );
}

#[test]
fn test_null_unbind_no_warning() {
    let file = make_file(vec![("Chat", vec![("ctrl+t", None)])]);
    let warnings = validate_file(&file);
    // null unbind should not produce invalid-action warning.
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidAction { .. }))
    );
}

#[test]
fn test_validation_warning_display() {
    let w = ValidationWarning::InvalidContext {
        name: "Bad".to_string(),
    };
    assert!(w.to_string().contains("Bad"));
}

#[test]
fn test_unknown_namespace_warning() {
    let file = make_file(vec![("Chat", vec![("ctrl+t", Some("bogus:action"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::UnknownNamespace { .. }))
    );
}

#[test]
fn test_known_namespace_no_warning() {
    let file = make_file(vec![(
        "Chat",
        vec![("ctrl+t", Some("ext:cycleThinkingLevel"))],
    )]);
    let warnings = validate_file(&file);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::UnknownNamespace { .. }))
    );
}

#[test]
fn test_platform_reserved_key_macos() {
    let file = make_file(vec![("Chat", vec![("meta+c", Some("chat:cancel"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::PlatformReservedKey { .. }))
    );
}

#[test]
fn test_ctrl_backslash_reserved() {
    let file = make_file(vec![("Chat", vec![("ctrl+\\", Some("chat:cancel"))])]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::ReservedKey { .. }))
    );
}

#[test]
fn test_chord_length_exceeded() {
    // 5-key chord exceeds MAX_CHORD_LENGTH of 4
    let file = make_file(vec![(
        "Chat",
        vec![("ctrl+a ctrl+b ctrl+c ctrl+d ctrl+e", Some("app:interrupt"))],
    )]);
    let warnings = validate_file(&file);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::ChordLengthExceeded { .. }))
    );
}

#[test]
fn test_chord_length_ok() {
    // 2-key chord is fine
    let file = make_file(vec![(
        "Chat",
        vec![("ctrl+k ctrl+c", Some("ext:clearScreen"))],
    )]);
    let warnings = validate_file(&file);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::ChordLengthExceeded { .. }))
    );
}
