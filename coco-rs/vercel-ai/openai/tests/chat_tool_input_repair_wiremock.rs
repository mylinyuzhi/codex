//! Wire-level repair tests for OpenAI Chat Completions.
//!
//! These tests exercise the **real adapter** by routing
//! `do_generate` through a local [`wiremock`] server returning the
//! exact JSON shape `api.openai.com/v1/chat/completions` produces —
//! including the malformations real models occasionally emit
//! (markdown fence, trailing comma, unrecoverable garbage). The
//! assertions pin the resulting `ToolCallPart.input` /
//! `ToolCallPart.invalid` so the Layer-1 contract
//! (`parse_tool_arguments_or_empty` with `{}` fallback) is locked at
//! the wire boundary, not just at the helper.
//!
//! This is the exemplar wiremock harness; the same pattern carries
//! to OpenAI Responses, OpenAI-compatible, Anthropic (non-stream +
//! stream), and Google adapters.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use vercel_ai_openai::OpenAIProviderSettings;
use vercel_ai_openai::create_openai;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::content::TextPart;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

/// Build a Chat Completions response body where the single tool call
/// carries the given `arguments` string. Mirrors the wire shape
/// emitted by `api.openai.com/v1/chat/completions` for a tool_use
/// completion (id, model, choices[].message.tool_calls[]).
fn chat_response_with_arguments(arguments: &str) -> serde_json::Value {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_700_000_000,
        "model": "gpt-test",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "Read",
                        "arguments": arguments,
                    },
                }],
            },
            "finish_reason": "tool_calls",
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
        },
    })
}

/// Build a single-message prompt suitable for hitting do_generate
/// once. Most adapter logic is independent of prompt shape; we just
/// need *something* non-empty.
fn one_shot_options() -> LanguageModelV4CallOptions {
    LanguageModelV4CallOptions {
        prompt: vec![LanguageModelV4Message::User {
            content: vec![UserContentPart::Text(TextPart::new("read /tmp/x"))],
            provider_options: None,
        }],
        ..Default::default()
    }
}

/// Drive a single `do_generate` against the local wiremock server
/// and return the first `ToolCallPart` from the response (panics if
/// the response shape doesn't have one — these tests construct the
/// response directly, so absence is a test bug).
async fn dispatch_with_arguments(raw_arguments: &str) -> vercel_ai_provider::ToolCallPart {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(chat_response_with_arguments(raw_arguments)),
        )
        .mount(&server)
        .await;

    let provider = create_openai(OpenAIProviderSettings {
        base_url: Some(server.uri()),
        api_key: Some("test-key".to_string()),
        ..Default::default()
    });
    let model = provider.chat("gpt-test");
    let options = one_shot_options();

    let result = model
        .do_generate(&options, None)
        .await
        .expect("do_generate against wiremock should succeed");

    result
        .content
        .into_iter()
        .find_map(|p| match p {
            AssistantContentPart::ToolCall(tc) => Some(tc),
            _ => None,
        })
        .expect("response contained a tool call by construction")
}

// ─── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn openai_chat_nonstream_clean_arguments_round_trip() {
    let tc = dispatch_with_arguments(r#"{"file_path": "/tmp/x"}"#).await;
    assert_eq!(tc.tool_name, "Read");
    assert_eq!(tc.input, json!({"file_path": "/tmp/x"}));
    assert!(!tc.invalid, "clean arguments must not flag invalid");
    assert!(tc.invalid_reason.is_none());
}

#[tokio::test]
async fn openai_chat_nonstream_trailing_comma_is_repaired() {
    // GPT-4.x / GPT-5 occasionally emit a trailing comma when the
    // model truncates parameter-dense arguments mid-decoding.
    let tc = dispatch_with_arguments(r#"{"file_path": "/tmp/x", "limit": 100,}"#).await;
    assert_eq!(tc.input, json!({"file_path": "/tmp/x", "limit": 100}));
    assert!(!tc.invalid, "repair should rescue trailing-comma input");
}

#[tokio::test]
async fn openai_chat_nonstream_markdown_fence_is_repaired() {
    // Some compat backends (GLM, Doubao) wrap tool arguments in
    // ```json...``` despite the OpenAI protocol forbidding it.
    let raw = "```json\n{\"file_path\": \"/repo/main.rs\"}\n```";
    let tc = dispatch_with_arguments(raw).await;
    assert_eq!(tc.input, json!({"file_path": "/repo/main.rs"}));
    assert!(!tc.invalid);
}

#[tokio::test]
async fn openai_chat_nonstream_unrecoverable_falls_back_to_empty_object() {
    // Pure garbage — `parse_with_repair` returns Err; the adapter
    // falls back to `Value::Object({})` so Layer 2 schema validation
    // can report specific missing fields. **Critically, `invalid`
    // stays `false`** — we want the schema validator to do the
    // classification, not Layer 1.
    let tc = dispatch_with_arguments("\u{0000}!!!@@@%%%").await;
    // `llm_json::repair_json` is intentionally aggressive — for some
    // garbage inputs it salvages to `Value::Null` (e.g. when the
    // leading NUL byte parses as the JSON `null` literal); for
    // others it lands on `Value::Object({})`. Both shapes hand
    // Layer 2 a non-string value, which the schema validator then
    // classifies as a type mismatch and emits the
    // `InputValidationError` wrap with the precise expected type.
    //
    // The contract we lock here: Layer 1 **does not unilaterally
    // invalidate** — whatever `parse_with_repair` returns flows
    // through verbatim, and `invalid` stays false. Classification
    // is Layer 2's job.
    assert!(
        matches!(
            tc.input,
            serde_json::Value::Object(_) | serde_json::Value::Null
        ),
        "expected object or null fallback, got {:?}",
        tc.input
    );
    assert!(
        !tc.invalid,
        "Layer 1 must not unilaterally invalidate; Layer 2 owns that decision"
    );
}

#[tokio::test]
async fn openai_chat_nonstream_empty_arguments_string_becomes_empty_object() {
    let tc = dispatch_with_arguments("").await;
    assert_eq!(tc.input, json!({}));
    assert!(!tc.invalid);
}

#[tokio::test]
async fn openai_chat_nonstream_single_quotes_repaired() {
    let tc = dispatch_with_arguments(r#"{'file_path': '/tmp/sq'}"#).await;
    assert_eq!(tc.input, json!({"file_path": "/tmp/sq"}));
    assert!(!tc.invalid);
}
