use super::*;
use crate::state::SymbolSuggestionItem;

fn create_test_state() -> SymbolSuggestionState {
    let mut state = SymbolSuggestionState::new("ModelInfo".to_string(), 0);
    state.update_suggestions(vec![
        SymbolSuggestionItem {
            name: "ModelInfo".to_string(),
            kind: SymbolKind::Struct,
            file_path: "protocol/src/model.rs".to_string(),
            line: 42,
            score: 100,
            match_indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
        },
        SymbolSuggestionItem {
            name: "model_info_new".to_string(),
            kind: SymbolKind::Function,
            file_path: "api/src/client.rs".to_string(),
            line: 156,
            score: 80,
            match_indices: vec![0, 1, 2, 3, 4],
        },
    ]);
    state
}

#[test]
fn test_popup_creation() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = SymbolSuggestionPopup::new(&state, &theme);

    let input_area = Rect::new(0, 20, 80, 3);
    let area = popup.calculate_area(input_area, 24);

    assert!(area.width >= 30);
    assert!(area.height >= 3);
}

#[test]
fn test_popup_render() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = SymbolSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    // Should contain the query
    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("ModelInfo"));
}

#[test]
fn test_empty_suggestions() {
    let mut state = SymbolSuggestionState::new("xyz".to_string(), 0);
    state.update_suggestions(vec![]);
    let theme = Theme::default();
    let popup = SymbolSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("No matching"));
}

#[test]
fn test_loading_state() {
    let state = SymbolSuggestionState::new("test".to_string(), 0);
    // state.loading is true by default
    let theme = Theme::default();
    let popup = SymbolSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("Indexing"));
}
