use super::*;

#[test]
fn test_get_editor_returns_value() {
    // Just test that get_editor returns a non-empty value
    // (either from env or the default "vim")
    let editor = get_editor();
    assert!(!editor.is_empty());
}

#[test]
fn test_edit_result_struct() {
    let result = EditResult {
        content: "test content".to_string(),
        modified: true,
    };
    assert!(result.modified);
    assert_eq!(result.content, "test content");
}
