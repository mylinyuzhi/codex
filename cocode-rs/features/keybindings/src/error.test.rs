use super::*;

#[test]
fn test_parse_keystroke_error_display() {
    let err = KeybindingError::ParseKeystroke {
        input: "ctrl++".to_string(),
        reason: "empty segment".to_string(),
    };
    assert!(err.to_string().contains("ctrl++"));
    assert!(err.to_string().contains("empty segment"));
}
