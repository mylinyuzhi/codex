use super::*;

use std::sync::Arc;

use coco_messages::AssistantContent;
use coco_messages::ReasoningContent;
use coco_messages::TextContent;
use coco_messages::create_assistant_message;
use coco_types::ModelRole;

use crate::i18n::locale_test_guard;
use crate::state::AppState;
use crate::state::derive::test_helpers;

#[test]
fn status_bar_view_renders_model_tokens_context_and_messages() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.provider = "openai".into();
    state.session.model = "gpt-5.2".into();
    state.session.session_usage = Some(coco_types::SessionUsageSnapshot {
        totals: coco_types::SessionUsageTotals {
            input_tokens: 1_500,
            output_tokens: 250,
            cache_read_input_tokens: 750,
            input_cost_usd: 0.0030,
            output_cost_usd: 0.0030,
            cache_read_cost_usd: 0.0012,
            total_cost_usd: 0.0072,
            ..Default::default()
        },
        ..Default::default()
    });
    state.session.token_usage.input_tokens = 1_500;
    state.session.token_usage.output_tokens = 250;
    state.session.token_usage.cache_read_tokens = 750;
    state.session.model_by_role.insert(
        ModelRole::Main,
        crate::state::ModelBinding {
            provider: "openai".into(),
            model_id: "gpt-5.2".into(),
            context_window: Some(100),
            effort: None,
        },
    );
    state
        .session
        .transcript
        .on_message_appended(Arc::new(create_assistant_message(
            vec![AssistantContent::Text(TextContent {
                text: "done".into(),
                provider_metadata: None,
            })],
            "gpt-5.2",
            coco_types::TokenUsage {
                input_tokens: coco_types::InputTokens {
                    total: 70,
                    ..Default::default()
                },
                output_tokens: coco_types::OutputTokens {
                    total: 10,
                    ..Default::default()
                },
            },
        )));

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains(" openai/gpt-5.2"));
    assert!(text.contains("↑1.5K/$0.0042 ↓250/$0.0030"));
    assert!(text.contains("cache 750/50%"));
    assert!(!text.contains("F2 to expand"));
    assert!(text.contains("ctx 80%"));
    assert!(text.contains("→0 ←1"));
    assert!(
        spans
            .iter()
            .any(|span| span.text == "ctx 80%" && span.tone == StatusTone::Warning)
    );
}

#[test]
fn status_bar_view_renders_lsp_badge() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.lsp_active = true;

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("LSP"));
}

#[test]
fn status_bar_view_renders_unknown_context_without_assistant_usage() {
    let _locale = locale_test_guard("en");
    let state = AppState::default();

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("ctx --"));
}

#[test]
fn status_bar_view_renders_total_input_tokens_and_cache_breakdown() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.token_usage.input_tokens = 5_020_000;
    state.session.token_usage.output_tokens = 14_800;
    state.session.token_usage.cache_read_tokens = 4_600_000;

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("↑5.0M ↓14.8K · cache 4.6M/91%"));
}

#[test]
fn status_bar_view_counts_transcript_messages_by_uuid_and_role() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    test_helpers::push_user_text(&mut state.session, "u1", "hello");
    let assistant = create_assistant_message(
        vec![
            AssistantContent::Reasoning(ReasoningContent::new("thinking")),
            AssistantContent::Text(TextContent::new("answer")),
        ],
        "test-model",
        coco_types::TokenUsage::default(),
    );
    state
        .session
        .transcript
        .on_message_appended(Arc::new(assistant));
    test_helpers::push_tool_result(&mut state.session, "call-1", "Glob", "done", false);

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("→1 ←1 · tool 1"));
}

#[test]
fn custom_status_line_replaces_built_in_segments() {
    let mut state = AppState::default();
    state.ui.display_settings.status_line = Some(coco_config::StatusLineSettings::Command(
        coco_config::StatusLineCommandSettings {
            command: "printf custom".to_string(),
            padding: 0,
        },
    ));
    state.ui.status_line.apply_update(StatusLineUpdate {
        generation: 0,
        output: Some("custom\nsecond".to_string()),
    });

    let StatusBarView::Custom { line } = status_bar_view(&state) else {
        panic!("expected custom status bar");
    };

    assert_eq!(line, "custom");
}

#[test]
fn exit_prompt_takes_precedence_over_custom_status_line() {
    let mut state = AppState::default();
    state.ui.display_settings.status_line = Some(coco_config::StatusLineSettings::Command(
        coco_config::StatusLineCommandSettings {
            command: "printf custom".to_string(),
            padding: 0,
        },
    ));
    state.ui.status_line.apply_update(StatusLineUpdate {
        generation: 0,
        output: Some("custom".to_string()),
    });
    state.ui.ctrl_c_tracker.poll((), std::time::Instant::now());

    let StatusBarView::ExitPrompt { key, text } = status_bar_view(&state) else {
        panic!("expected exit prompt");
    };

    assert_eq!(key, crate::state::ExitKey::CtrlC);
    assert!(text.contains("Ctrl-C"));
}

#[test]
fn built_in_status_preserves_pending_chord_hint() {
    use crate::keybinding_bridge::KeybindingContext;
    use crate::keybinding_resolver::ResolverResult;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyEventKind;
    use crossterm::event::KeyEventState;
    use crossterm::event::KeyModifiers;

    let state = AppState::default();
    let result = state.ui.kb_handle.resolve_key(
        KeyEvent {
            code: KeyCode::Char('x'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        },
        KeybindingContext::Chat,
    );
    assert!(matches!(result, ResolverResult::Pending));

    let StatusBarView::BuiltIn { spans } = status_bar_view(&state) else {
        panic!("expected built-in status bar");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("ctrl+x"));
    assert!(text.contains("…"));
}
