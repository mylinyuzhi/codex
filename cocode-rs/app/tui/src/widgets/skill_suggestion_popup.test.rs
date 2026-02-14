use super::*;
use crate::state::SkillSuggestionItem;

fn create_test_state() -> SkillSuggestionState {
    let mut state = SkillSuggestionState::new("com".to_string(), 0);
    state.update_suggestions(vec![
        SkillSuggestionItem {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
            score: -100,
            match_indices: vec![0, 1, 2],
        },
        SkillSuggestionItem {
            name: "config".to_string(),
            description: "Configure settings".to_string(),
            score: -98,
            match_indices: vec![0, 1],
        },
    ]);
    state
}

#[test]
fn test_popup_creation() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = SkillSuggestionPopup::new(&state, &theme);

    let input_area = Rect::new(0, 20, 80, 3);
    let area = popup.calculate_area(input_area, 24);

    assert!(area.width >= 30);
    assert!(area.height >= 3);
}

#[test]
fn test_popup_render() {
    let state = create_test_state();
    let theme = Theme::default();
    let popup = SkillSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    // Should contain the query
    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("com"));
}

#[test]
fn test_empty_suggestions() {
    let mut state = SkillSuggestionState::new("xyz".to_string(), 0);
    state.update_suggestions(vec![]);
    let theme = Theme::default();
    let popup = SkillSuggestionPopup::new(&state, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("No matching"));
}
