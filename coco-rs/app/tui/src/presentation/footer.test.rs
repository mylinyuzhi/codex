use super::*;

use std::sync::Arc;

use coco_messages::AssistantContent;
use coco_messages::ReasoningContent;
use coco_messages::TextContent;
use coco_messages::create_assistant_message;

use crate::i18n::locale_test_guard;
use crate::state::AppState;
use crate::state::derive::test_helpers;

#[test]
fn footer_view_renders_model_tokens_context_and_messages() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.provider = "openai".into();
    state.session.model = "gpt-5.2".into();
    state.session.token_usage.input_tokens = 1_500;
    state.session.token_usage.output_tokens = 250;
    state.session.token_usage.cache_read_tokens = 750;
    state.session.context_window_used = 80;
    state.session.context_window_total = 100;

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains(" openai/gpt-5.2"));
    assert!(text.contains("↑1.5K ↓250"));
    assert!(text.contains("cache 750/50%"));
    assert!(!text.contains("thinking hidden (F2)"));
    assert!(text.contains("ctx 80%"));
    assert!(text.contains("→0 ←0"));
    assert!(
        spans
            .iter()
            .any(|span| span.text == "ctx 80%" && span.tone == FooterTone::Warning)
    );
}

#[test]
fn footer_view_renders_total_input_tokens_and_cache_breakdown() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.token_usage.input_tokens = 5_020_000;
    state.session.token_usage.output_tokens = 14_800;
    state.session.token_usage.cache_read_tokens = 4_600_000;

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("↑5.0M ↓14.8K · cache 4.6M/91%"));
}

#[test]
fn footer_view_counts_transcript_messages_by_uuid_and_role() {
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

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("→1 ←1 · tool 1"));
}

#[test]
fn footer_view_omits_show_thinking_on_state() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.ui.show_thinking = true;

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(!text.contains("thinking visible (F2)"));
}
