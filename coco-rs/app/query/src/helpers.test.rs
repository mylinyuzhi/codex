use super::*;
use coco_messages::Message;
use coco_messages::StopReason;

fn api_error_text(msg: &Message) -> String {
    let Message::Assistant(asst) = msg else {
        panic!("expected Assistant message, got {msg:?}");
    };
    asst.api_error
        .as_ref()
        .map(|e| e.message.clone())
        .unwrap_or_default()
}

#[test]
fn api_error_message_max_tokens_includes_configured_cap() {
    let msg = build_abnormal_stop_api_error_message(StopReason::MaxTokens, Some(8_000));
    let text = api_error_text(&msg);
    assert!(
        text.contains("8000"),
        "configured max_tokens must appear in user-facing text: {text}"
    );
    assert!(text.starts_with("API Error"));
}

#[test]
fn api_error_message_max_tokens_without_cap_falls_back() {
    let msg = build_abnormal_stop_api_error_message(StopReason::MaxTokens, None);
    let text = api_error_text(&msg);
    assert!(text.contains("output token maximum"));
}

#[test]
fn api_error_message_context_window_exceeded_is_distinct_variant() {
    let msg = build_abnormal_stop_api_error_message(StopReason::ContextWindowExceeded, Some(8_000));
    let text = api_error_text(&msg);
    assert!(
        text.contains("context window"),
        "context-window-exceeded must distinguish from plain max_tokens: {text}"
    );
    assert!(
        !text.contains("8000"),
        "context-window message must not show output-token cap (would mislead the user about which limit was hit): {text}"
    );
}

#[test]
fn api_error_message_content_filter_is_provider_agnostic() {
    let msg = build_abnormal_stop_api_error_message(StopReason::ContentFilter, None);
    let text = api_error_text(&msg);
    assert!(text.contains("declined"));
    assert!(
        text.contains("content policy") || text.contains("safety"),
        "must mention the policy/safety bucket so it covers refusal / SAFETY / RECITATION: {text}"
    );
    // Provider-agnostic — no "Claude" / "Anthropic" / "OpenAI" mentions.
    assert!(!text.contains("Claude"));
    assert!(!text.contains("Anthropic"));
}

#[test]
fn api_error_message_has_empty_content_carry() {
    // The synthetic message body is empty content + api_error field.
    // The real partial assistant message is pushed separately by the
    // engine; this one is purely the typed signal.
    let msg = build_abnormal_stop_api_error_message(StopReason::ContentFilter, None);
    let Message::Assistant(asst) = &msg else {
        panic!("expected Assistant message");
    };
    let coco_inference::LanguageModelMessage::Assistant { content, .. } = &asst.message else {
        panic!("expected Assistant LlmMessage variant");
    };
    assert!(
        content.is_empty(),
        "synthetic message must have empty content — the real partial \
         response is pushed separately by the engine"
    );
    assert!(asst.api_error.is_some(), "api_error field must be set");
}

#[test]
fn api_error_message_unspecified_variant_uses_wire_str() {
    // `Error` / `Other` shouldn't normally reach this builder (engine
    // only invokes it for ContentFilter / MaxTokens / ContextWindowExceeded),
    // but if it does the fallback should not panic and should
    // surface the typed variant by its wire name.
    let msg = build_abnormal_stop_api_error_message(StopReason::Error, None);
    let text = api_error_text(&msg);
    assert!(
        text.contains("error"),
        "fallback should name the variant: {text}"
    );
}
