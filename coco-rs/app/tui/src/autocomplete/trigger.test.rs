use super::apply_async_result;
use super::detect;
use super::refresh_suggestions;
use crate::state::ActiveSuggestions;
use crate::state::AppState;
use crate::state::SlashCommandInfo;
use crate::state::SuggestionKind;
use crate::widgets::suggestion_popup::SuggestionItem;
use crate::widgets::suggestion_popup::SuggestionMeta;

fn slash(name: &str, description: Option<&str>) -> SlashCommandInfo {
    SlashCommandInfo {
        name: name.to_string(),
        description: description.map(ToString::to_string),
        ..SlashCommandInfo::default()
    }
}

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
fn test_at_trigger_unified() {
    // After the @-trigger unification, `@src` produces a single `At`
    // kind — the popup decides per-row whether each suggestion is a
    // file path, an agent, or an MCP resource.
    let t = detect("hello @src", 10).expect("at trigger detected");
    assert_eq!(t.kind, SuggestionKind::At);
    assert_eq!(t.pos, 6);
    assert_eq!(t.query, "src");
}

#[test]
fn test_at_bare_word_no_agent_subprefix() {
    // The legacy `@agent-<name>` sub-prefix is gone. `@agent-plan`
    // becomes a unified `@` query whose body happens to start with
    // `agent-` — agents fall out of `seed_agent_items` by fuzzy match
    // on the bare name, not by prefix.
    let t = detect("@agent-plan", 11).expect("at trigger detected");
    assert_eq!(t.kind, SuggestionKind::At);
    assert_eq!(t.query, "agent-plan");
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
fn test_cjk_at_trigger_uses_byte_offset() {
    // "你好" is 6 bytes (3 per CJK char). A trigger at byte 6 puts `@`
    // immediately after the 2-char prefix; the splice site must be the
    // byte offset, not the char index.
    let text = "你好 @src";
    // After "你好 " is 7 bytes, then "@src" starts at byte 7.
    let cursor = text.len(); // end of input
    let t = detect(text, cursor).expect("trigger detected after CJK prefix");
    assert_eq!(t.kind, SuggestionKind::At);
    assert_eq!(t.pos, 7, "trigger pos must be byte 7 (not char 3)");
    assert_eq!(t.query, "src");
}

#[test]
fn test_refresh_installs_slash_suggestions() {
    let mut state = AppState::new();
    state.session.available_commands = vec![
        slash("help", Some("Show help")),
        slash("clear", None),
        slash("config", Some("Edit settings")),
    ];
    state.ui.input.textarea.set_text("/c");
    state.ui.input.textarea.set_cursor(2);

    refresh_suggestions(&mut state);

    let sug = state.ui.active_suggestions.expect("popup installed");
    assert_eq!(sug.kind, SuggestionKind::SlashCommand);
    assert_eq!(sug.query, "c");
    let labels: Vec<&str> = sug.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["/clear", "/config"]);
}

#[test]
fn test_refresh_dismisses_when_space_typed() {
    let mut state = AppState::new();
    state.session.available_commands = vec![slash("help", None)];
    state.ui.input.textarea.set_text("/help");
    state.ui.input.textarea.set_cursor(5);
    refresh_suggestions(&mut state);
    assert!(state.ui.active_suggestions.is_some());

    state.ui.input.textarea.set_text("/help ");
    state.ui.input.textarea.set_cursor(6);
    refresh_suggestions(&mut state);
    assert!(state.ui.active_suggestions.is_none());
}

#[test]
fn test_refresh_seeds_at_trigger_with_agents() {
    // The unified `@` trigger seeds the popup with agent matches
    // synchronously; async file results merge in later via
    // `apply_async_result`. The empty-query case shows every agent.
    let mut state = AppState::new();
    state.session.available_agents = vec![
        crate::autocomplete::AgentInfo {
            name: "plan".into(),
            agent_type: "planner".into(),
            description: Some("Planning agent".into()),
            color: None,
        },
        crate::autocomplete::AgentInfo {
            name: "review".into(),
            agent_type: "reviewer".into(),
            description: None,
            color: None,
        },
    ];
    state.ui.input.textarea.set_text("@pl");
    state.ui.input.textarea.set_cursor(3);
    refresh_suggestions(&mut state);

    let sug = state.ui.active_suggestions.expect("popup installed");
    assert_eq!(sug.kind, SuggestionKind::At);
    assert_eq!(sug.query, "pl");
    let labels: Vec<&str> = sug.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["plan (agent)"]);
}

#[test]
fn test_apply_async_result_merges_files_after_agents() {
    let mut state = AppState::new();
    state.session.available_agents = vec![crate::autocomplete::AgentInfo {
        name: "src".into(),
        agent_type: "src".into(),
        description: Some("Source explorer".into()),
        color: None,
    }];
    state.ui.input.textarea.set_text("@src");
    state.ui.input.textarea.set_cursor(4);
    refresh_suggestions(&mut state);
    // Popup seeded with 1 agent already.
    assert_eq!(state.ui.active_suggestions.as_ref().unwrap().items.len(), 1);

    let file_results = vec![
        SuggestionItem {
            label: "src/lib.rs".into(),
            description: None,
            metadata: Some(SuggestionMeta::Path {
                is_directory: false,
            }),
        },
        SuggestionItem {
            label: "src/main.rs".into(),
            description: None,
            metadata: Some(SuggestionMeta::Path {
                is_directory: false,
            }),
        },
    ];
    let adopted = apply_async_result(&mut state, SuggestionKind::At, "src", file_results);
    assert!(adopted);

    let sug = state.ui.active_suggestions.as_ref().unwrap();
    // Agent first, then both files (merge preserves order, cap 15).
    assert_eq!(sug.items.len(), 3);
    assert!(matches!(
        sug.items[0].metadata,
        Some(SuggestionMeta::Agent { .. })
    ));
    assert!(matches!(
        sug.items[1].metadata,
        Some(SuggestionMeta::Path { .. })
    ));
}

#[test]
fn test_apply_async_result_drops_stale_query() {
    // User moved on to a new query before the search returned. Slow
    // result must not clobber the current trigger state.
    let mut state = AppState::new();
    state.ui.active_suggestions = Some(ActiveSuggestions {
        kind: SuggestionKind::At,
        items: Vec::new(),
        selected: 0,
        query: "docs".into(),
        trigger_pos: 0,
    });

    let adopted = apply_async_result(
        &mut state,
        SuggestionKind::At,
        "src", // stale — user has moved to "docs"
        vec![SuggestionItem {
            label: "src/lib.rs".into(),
            description: None,
            metadata: Some(SuggestionMeta::Path {
                is_directory: false,
            }),
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
            metadata: None,
        }],
    );
    assert!(!adopted);
    assert!(state.ui.active_suggestions.is_none());
}
