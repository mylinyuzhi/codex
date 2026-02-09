use super::*;
use crate::state::AgentSuggestionItem;

fn create_test_state() -> AgentSuggestionState {
    let mut state = AgentSuggestionState::new("agent-exp".to_string(), 0);
    state.update_suggestions(vec![
        AgentSuggestionItem {
            agent_type: "explore".to_string(),
            name: "Explore".to_string(),
            description: "Read-only codebase exploration".to_string(),
            score: -100,
            match_indices: vec![0, 1, 2],
        },
        AgentSuggestionItem {
            agent_type: "general-purpose".to_string(),
            name: "General Purpose".to_string(),
            description: "General-purpose agent".to_string(),
            score: -50,
            match_indices: vec![],
        },
    ]);
    state
}

#[test]
fn test_popup_creation() {
    let state = create_test_state();
    let popup = AgentSuggestionPopup::new(&state);

    let input_area = Rect::new(0, 20, 80, 3);
    let area = popup.calculate_area(input_area, 24);

    assert!(area.width >= 30);
    assert!(area.height >= 3);
}

#[test]
fn test_popup_render() {
    let state = create_test_state();
    let popup = AgentSuggestionPopup::new(&state);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    // Should contain the query
    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("agent-exp"));
}

#[test]
fn test_empty_suggestions() {
    let mut state = AgentSuggestionState::new("agent-xyz".to_string(), 0);
    state.update_suggestions(vec![]);
    let popup = AgentSuggestionPopup::new(&state);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    popup.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("No matching"));
}
