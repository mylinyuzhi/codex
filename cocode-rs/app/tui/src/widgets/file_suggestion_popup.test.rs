use super::*;
use crate::state::FileSuggestionItem;

fn create_test_state() -> FileSuggestionState {
    let mut state = FileSuggestionState::new("src/".to_string(), 0);
    state.update_suggestions(vec![
        FileSuggestionItem {
            path: "src/main.rs".to_string(),
            display_text: "src/main.rs".to_string(),
            score: 100,
            match_indices: vec![0, 1, 2, 4, 5, 6, 7],
            is_directory: false,
        },
        FileSuggestionItem {
            path: "src/lib.rs".to_string(),
            display_text: "src/lib.rs".to_string(),
            score: 90,
            match_indices: vec![0, 1, 2],
            is_directory: false,
        },
        FileSuggestionItem {
            path: "src/utils/".to_string(),
            display_text: "src/utils".to_string(),
            score: 80,
            match_indices: vec![0, 1, 2],
            is_directory: true,
        },
    ]);
    state
}

#[test]
fn test_popup_creation() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = FileSuggestionPopup::new(&state, &theme);

    let input_area = Rect::new(0, 20, 80, 3);
    let area = popup.calculate_area(input_area, 24);

    assert!(area.width >= 30);
    assert!(area.height >= 3);
}

#[test]
fn test_popup_render() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = FileSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    // Should contain the query
    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(content.contains("src/"));
}

#[test]
fn test_highlight_path() {
    let path = "src/main.rs";
    let indices = vec![0, 4, 5];

    let line = FileSuggestionPopup::highlight_path(path, &indices);

    // Should have multiple spans (highlighted and non-highlighted)
    assert!(!line.spans.is_empty());
}

#[test]
fn test_empty_suggestions() {
    let mut state = FileSuggestionState::new("xyz".to_string(), 0);
    // Update with empty suggestions to mark loading as false
    state.update_suggestions(vec![]);
    let theme = Theme::default();
    let popup = FileSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(content.contains("No matches"));
}

#[test]
fn test_loading_state() {
    let state = FileSuggestionState::new("src".to_string(), 0);
    // loading is true by default
    let theme = Theme::default();
    let popup = FileSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(content.contains("Searching"));
}
