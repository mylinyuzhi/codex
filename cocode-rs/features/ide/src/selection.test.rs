use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[tokio::test]
async fn test_update_with_selection_range() {
    let state = IdeSelectionState::new();

    let params = json!({
        "selection": {
            "start": {"line": 5, "character": 0},
            "end": {"line": 8, "character": 10}
        },
        "text": "selected code",
        "filePath": "/src/main.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");

    assert_eq!(sel.file_path.to_string_lossy(), "/src/main.rs");
    assert_eq!(sel.text.as_deref(), Some("selected code"));
    assert_eq!(sel.line_start, 5);
    assert_eq!(sel.line_count, 4); // lines 5,6,7,8
    assert!(sel.has_selection());
}

#[tokio::test]
async fn test_update_end_character_zero_adjustment() {
    let state = IdeSelectionState::new();

    // When end.character == 0, the cursor is at the start of the next line
    // without selecting any text on that line.
    let params = json!({
        "selection": {
            "start": {"line": 10, "character": 0},
            "end": {"line": 13, "character": 0}
        },
        "filePath": "/src/lib.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");
    assert_eq!(sel.line_count, 3); // lines 10,11,12 (not 13)
}

#[tokio::test]
async fn test_update_file_only_context() {
    let state = IdeSelectionState::new();

    let params = json!({
        "text": "file content",
        "filePath": "/src/main.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");

    assert_eq!(sel.file_path.to_string_lossy(), "/src/main.rs");
    assert_eq!(sel.line_count, 0);
    assert!(!sel.has_selection());
}

#[tokio::test]
async fn test_update_clears_on_empty() {
    let state = IdeSelectionState::new();

    // First set a selection
    let params = json!({
        "filePath": "/src/main.rs",
        "selection": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 1, "character": 0}
        }
    });
    state.update_from_notification(&params).await;
    assert!(state.get().await.is_some());

    // Clear explicitly
    state.clear().await;
    assert!(state.get().await.is_none());
}

#[tokio::test]
async fn test_update_no_file_path_clears() {
    let state = IdeSelectionState::new();

    let params = json!({});
    state.update_from_notification(&params).await;
    assert!(state.get().await.is_none());
}

#[tokio::test]
async fn test_update_single_line_cursor_position() {
    let state = IdeSelectionState::new();

    // Cursor positioned on a single line (zero-width selection).
    // Matches Claude Code behavior: lineCount stays 1 because
    // the `end.character == 0 && lineCount > 1` check does not apply.
    let params = json!({
        "selection": {
            "start": {"line": 5, "character": 0},
            "end": {"line": 5, "character": 0}
        },
        "filePath": "/src/main.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");
    assert_eq!(sel.line_count, 1);
    assert!(sel.has_selection());
}

#[tokio::test]
async fn test_update_single_line_with_width() {
    let state = IdeSelectionState::new();

    let params = json!({
        "selection": {
            "start": {"line": 10, "character": 5},
            "end": {"line": 10, "character": 20}
        },
        "text": "selected text",
        "filePath": "/src/main.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");
    assert_eq!(sel.line_count, 1);
    assert_eq!(sel.text.as_deref(), Some("selected text"));
}

#[tokio::test]
async fn test_update_reversed_selection_range() {
    let state = IdeSelectionState::new();

    // Reversed range (end < start) — should clamp line_count to 0
    let params = json!({
        "selection": {
            "start": {"line": 10, "character": 5},
            "end": {"line": 5, "character": 0}
        },
        "filePath": "/src/main.rs"
    });

    state.update_from_notification(&params).await;
    let sel = state.get().await.expect("should have selection");
    assert_eq!(sel.line_count, 0);
    assert!(!sel.has_selection());
}

#[tokio::test]
async fn test_update_empty_file_path_clears() {
    let state = IdeSelectionState::new();

    // First set a selection
    let params = json!({
        "selection": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 1, "character": 0}
        },
        "filePath": "/src/main.rs"
    });
    state.update_from_notification(&params).await;
    assert!(state.get().await.is_some());

    // Empty file path should clear selection
    let params = json!({
        "filePath": "",
        "selection": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 1, "character": 0}
        }
    });
    state.update_from_notification(&params).await;
    assert!(state.get().await.is_none());
}
