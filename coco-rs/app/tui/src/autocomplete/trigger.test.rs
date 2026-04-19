use super::apply_async_result;
use super::detect;
use super::refresh_suggestions;
use crate::state::ActiveSuggestions;
use crate::state::AppState;
use crate::state::SuggestionKind;
use crate::widgets::suggestion_popup::SuggestionItem;

#[test]
fn test_slash_command_trigger() {
    let t = detect("/con", 4).expect("slash command detected");
    assert_eq!(t.kind, SuggestionKind::SlashCommand);
    assert_eq!(t.pos, 0);
    assert_eq!(t.query, "con");
}

#[test]
fn test_slash_with_space_is_not_trigger() {
    // Once the user has typed a space after the slash, we're in argument
    // territory and the suggestion popup should dismiss.
    assert!(detect("/help foo", 9).is_none());
}

#[test]
fn test_at_file_trigger() {
    let t = detect("hello @src", 10).expect("file trigger detected");
    assert_eq!(t.kind, SuggestionKind::File);
    assert_eq!(t.pos, 6);
    assert_eq!(t.query, "src");
}

#[test]
fn test_at_agent_trigger() {
    let t = detect("@agent-plan", 11).expect("agent trigger detected");
    assert_eq!(t.kind, SuggestionKind::Agent);
    assert_eq!(t.query, "plan");
}

#[test]
fn test_at_symbol_trigger() {
    let t = detect("@#foo", 5).expect("symbol trigger detected");
    assert_eq!(t.kind, SuggestionKind::Symbol);
    assert_eq!(t.query, "foo");
}

#[test]
fn test_email_does_not_trigger() {
    // `@` after non-whitespace must not start a suggestion — protects
    // `user@example.com` from being treated as a mention.
    assert!(detect("mylinyuzhi@gmail.com", 20).is_none());
}

#[test]
fn test_no_trigger_plain_text() {
    assert!(detect("hello world", 11).is_none());
}

#[test]
fn test_refresh_installs_slash_suggestions() {
    let mut state = AppState::new();
    state.session.available_commands = vec![
        ("help".to_string(), Some("Show help".to_string())),
        ("clear".to_string(), None),
        ("config".to_string(), Some("Edit settings".to_string())),
    ];
    state.ui.input.text = "/c".to_string();
    state.ui.input.cursor = 2;

    refresh_suggestions(&mut state);

    let sug = state.ui.active_suggestions.expect("popup installed");
    assert_eq!(sug.kind, SuggestionKind::SlashCommand);
    assert_eq!(sug.query, "c");
    // Filter contains `c` → clear + config match; help doesn't.
    let labels: Vec<&str> = sug.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["/clear", "/config"]);
}

#[test]
fn test_refresh_dismisses_when_space_typed() {
    let mut state = AppState::new();
    state.session.available_commands = vec![("help".to_string(), None)];
    state.ui.input.text = "/help".to_string();
    state.ui.input.cursor = 5;
    refresh_suggestions(&mut state);
    assert!(state.ui.active_suggestions.is_some());

    // User types a space → we're now in arguments; popup dismisses.
    state.ui.input.text = "/help ".to_string();
    state.ui.input.cursor = 6;
    refresh_suggestions(&mut state);
    assert!(state.ui.active_suggestions.is_none());
}

#[test]
fn test_refresh_installs_empty_file_trigger() {
    // File triggers install `active_suggestions` with empty items so the
    // App event loop can see the query and dispatch to FileSearchManager.
    // The Autocomplete context only activates once results arrive and
    // items is non-empty — until then the empty popup is invisible and
    // arrow keys keep passing through to input editing.
    let mut state = AppState::new();
    state.ui.input.text = "@src".to_string();
    state.ui.input.cursor = 4;
    refresh_suggestions(&mut state);

    let sug = state.ui.active_suggestions.expect("trigger installed");
    assert_eq!(sug.kind, SuggestionKind::File);
    assert_eq!(sug.query, "src");
    assert!(sug.items.is_empty());
}

#[test]
fn test_apply_async_result_updates_matching_query() {
    let mut state = AppState::new();
    state.ui.input.text = "@src".to_string();
    state.ui.input.cursor = 4;
    refresh_suggestions(&mut state);
    // Empty async trigger installed.
    assert!(
        state
            .ui
            .active_suggestions
            .as_ref()
            .unwrap()
            .items
            .is_empty()
    );

    let suggestions = vec![
        SuggestionItem {
            label: "src/lib.rs".into(),
            description: None,
        },
        SuggestionItem {
            label: "src/main.rs".into(),
            description: None,
        },
    ];
    let adopted = apply_async_result(&mut state, SuggestionKind::File, "src", suggestions);
    assert!(adopted);

    let sug = state.ui.active_suggestions.as_ref().unwrap();
    assert_eq!(sug.items.len(), 2);
    assert_eq!(sug.items[0].label, "src/lib.rs");
}

#[test]
fn test_apply_async_result_drops_stale_query() {
    // User moved on to a new query before the search returned. Slow
    // result must not clobber the current trigger state.
    let mut state = AppState::new();
    state.ui.active_suggestions = Some(ActiveSuggestions {
        kind: SuggestionKind::File,
        items: Vec::new(),
        selected: 0,
        query: "docs".into(),
        trigger_pos: 0,
    });

    let adopted = apply_async_result(
        &mut state,
        SuggestionKind::File,
        "src", // stale — user has moved to "docs"
        vec![SuggestionItem {
            label: "src/lib.rs".into(),
            description: None,
        }],
    );
    assert!(!adopted);
    assert!(
        state
            .ui
            .active_suggestions
            .as_ref()
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn test_apply_async_result_drops_when_dismissed() {
    // User dismissed the popup (Esc) before result came back.
    let mut state = AppState::new();
    state.ui.active_suggestions = None;

    let adopted = apply_async_result(
        &mut state,
        SuggestionKind::Symbol,
        "foo",
        vec![SuggestionItem {
            label: "foo".into(),
            description: None,
        }],
    );
    assert!(!adopted);
    assert!(state.ui.active_suggestions.is_none());
}

#[test]
fn test_refresh_agent_trigger_from_session() {
    // @agent-* triggers are synchronous — refresh filters through the
    // session's agent registry and populates items inline.
    let mut state = AppState::new();
    state.session.available_agents = vec![
        crate::autocomplete::AgentInfo {
            name: "plan".into(),
            agent_type: "planner".into(),
            description: Some("Planning agent".into()),
        },
        crate::autocomplete::AgentInfo {
            name: "review".into(),
            agent_type: "reviewer".into(),
            description: None,
        },
    ];
    state.ui.input.text = "@agent-pl".to_string();
    state.ui.input.cursor = 9;
    refresh_suggestions(&mut state);

    let sug = state.ui.active_suggestions.expect("agent popup installed");
    assert_eq!(sug.kind, SuggestionKind::Agent);
    let labels: Vec<&str> = sug.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["@agent-plan"]);
}
