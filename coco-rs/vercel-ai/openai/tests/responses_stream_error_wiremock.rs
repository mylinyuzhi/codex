//! Wire-level streaming error tests for the OpenAI Responses API.
//!
//! Drives `do_stream` against an SSE fixture whose payload is an
//! in-band `error` event — HTTP 200 followed by a stream-level error,
//! the exact failure shape that previously collapsed to the opaque
//! "Unknown error". Locks the contract that the emitted
//! `LanguageModelV4StreamPart::Error` carries actionable detail (the
//! provider code, or the raw payload) rather than the bare placeholder.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use futures::StreamExt;
use vercel_ai_openai::OpenAIAuth;
use vercel_ai_openai::OpenAIProviderSettings;
use vercel_ai_openai::create_openai;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::StreamError;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::content::TextPart;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn one_shot_options() -> LanguageModelV4CallOptions {
    LanguageModelV4CallOptions {
        prompt: vec![LanguageModelV4Message::User {
            content: vec![UserContentPart::Text(TextPart::new("hi"))],
            provider_options: None,
        }],
        ..Default::default()
    }
}

/// Drive `do_stream` against an SSE body and return the first
/// `StreamError` emitted. Panics if the stream finishes without one.
async fn dispatch_error_stream(sse_body: &str) -> StreamError {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(sse_body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let provider = create_openai(OpenAIProviderSettings {
        base_url: Some(server.uri()),
        auth: OpenAIAuth::ApiKey(Some("test-key".to_string())),
        ..Default::default()
    });
    let model = provider.responses("gpt-test");
    let options = one_shot_options();

    let mut stream_result = model
        .do_stream(&options, None)
        .await
        .expect("do_stream against wiremock should open");

    while let Some(part) = stream_result.stream.next().await {
        if let Ok(LanguageModelV4StreamPart::Error { error }) = part {
            return error;
        }
    }
    panic!("stream finished without an Error part");
}

#[tokio::test]
async fn responses_stream_error_without_message_falls_back_to_raw() {
    // Server-side failures frequently arrive with null message/code.
    // The mapping must not collapse to the opaque "Unknown error" — the
    // raw payload is the only signal we have, so it must reach the user.
    let body = "data: {\"type\":\"error\"}\n\ndata: [DONE]\n\n";
    let err = dispatch_error_stream(body).await;
    assert_ne!(err.message, "Unknown error");
    assert!(
        err.message.contains("OpenAI responses error"),
        "expected raw-fallback message, got: {}",
        err.message
    );
}

#[tokio::test]
async fn responses_stream_error_with_code_surfaces_code() {
    let body = "data: {\"type\":\"error\",\"code\":\"server_error\"}\n\ndata: [DONE]\n\n";
    let err = dispatch_error_stream(body).await;
    assert_eq!(err.code.as_deref(), Some("server_error"));
    assert!(
        err.message.contains("server_error"),
        "code should appear in the surfaced message: {}",
        err.message
    );
}

#[tokio::test]
async fn responses_stream_error_with_message_passes_through() {
    let body = "data: {\"type\":\"error\",\"message\":\"rate limit reached\",\"code\":\"rate_limit\"}\n\ndata: [DONE]\n\n";
    let err = dispatch_error_stream(body).await;
    assert_eq!(err.message, "rate limit reached");
    assert_eq!(err.code.as_deref(), Some("rate_limit"));
}
