use std::str::FromStr;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_as_str_roundtrip() {
    for ctx in KeybindingContext::ALL {
        let s = ctx.as_str();
        let parsed = KeybindingContext::from_str(s).unwrap();
        assert_eq!(*ctx, parsed);
    }
}

#[test]
fn test_case_insensitive_parsing() {
    assert_eq!(
        KeybindingContext::from_str("chat").unwrap(),
        KeybindingContext::Chat
    );
    assert_eq!(
        KeybindingContext::from_str("GLOBAL").unwrap(),
        KeybindingContext::Global
    );
    assert_eq!(
        KeybindingContext::from_str("autocomplete").unwrap(),
        KeybindingContext::Autocomplete
    );
}

#[test]
fn test_unknown_context() {
    assert!(KeybindingContext::from_str("Unknown").is_err());
    assert!(KeybindingContext::from_str("").is_err());
}

#[test]
fn test_all_contexts_count() {
    assert_eq!(KeybindingContext::ALL.len(), 18);
}

#[test]
fn test_display() {
    assert_eq!(KeybindingContext::Chat.to_string(), "Chat");
    assert_eq!(KeybindingContext::Global.to_string(), "Global");
    assert_eq!(
        KeybindingContext::HistorySearch.to_string(),
        "HistorySearch"
    );
}

#[test]
fn test_serde_roundtrip() {
    let ctx = KeybindingContext::Chat;
    let json = serde_json::to_string(&ctx).unwrap();
    assert_eq!(json, "\"Chat\"");
    let parsed: KeybindingContext = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ctx);
}
