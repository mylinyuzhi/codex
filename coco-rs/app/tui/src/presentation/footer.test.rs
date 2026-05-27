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
    assert!(text.contains("↑1.5K/$0.0042 ↓250/$0.0030"));
    assert!(text.contains("cache 750/50%"));
    assert!(!text.contains("F2 to expand"));
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
fn footer_view_renders_unknown_session_usage_pricing() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.token_usage.input_tokens = 100;
    state.session.token_usage.output_tokens = 25;
    state.session.session_usage = Some(coco_types::SessionUsageSnapshot {
        totals: coco_types::SessionUsageTotals {
            input_tokens: 100,
            output_tokens: 25,
            request_count: 1,
            unpriced_request_count: 1,
            ..Default::default()
        },
        models: vec![coco_types::SessionModelUsageEntry {
            provider: "unknown".into(),
            model_id: "mystery".into(),
            input_tokens: 100,
            output_tokens: 25,
            request_count: 1,
            unpriced_request_count: 1,
            unpriced_input_tokens: 100,
            unpriced_output_tokens: 25,
            priced: false,
            ..Default::default()
        }],
        unpriced_models: vec![coco_types::ProviderModelSelection {
            provider: "unknown".into(),
            model_id: "mystery".into(),
        }],
        ..Default::default()
    });

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("↑100/$? ↓25/$?"));
}

#[test]
fn footer_view_shows_known_cost_for_mixed_pricing() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.token_usage.input_tokens = 200;
    state.session.token_usage.output_tokens = 50;
    state.session.session_usage = Some(coco_types::SessionUsageSnapshot {
        totals: coco_types::SessionUsageTotals {
            input_tokens: 200,
            output_tokens: 50,
            input_cost_usd: 0.004,
            output_cost_usd: 0.006,
            total_cost_usd: 0.010,
            request_count: 2,
            unpriced_request_count: 1,
            ..Default::default()
        },
        models: vec![coco_types::SessionModelUsageEntry {
            provider: "anthropic".into(),
            model_id: "claude-sonnet-4-5".into(),
            request_count: 2,
            unpriced_request_count: 1,
            priced: false,
            ..Default::default()
        }],
        unpriced_models: vec![coco_types::ProviderModelSelection {
            provider: "anthropic".into(),
            model_id: "claude-sonnet-4-5".into(),
        }],
        ..Default::default()
    });

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains("↑200/$0.0040 ↓50/$0.0060"));
    assert!(text.contains("unpriced 1"));
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

    assert!(!text.contains("F2 to collapse"));
}
