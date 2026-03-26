//! Basic text generation tests via ApiClient + UnifiedStream::collect().

use anyhow::Result;
use cocode_inference::ApiClient;
use cocode_inference::LanguageModel;
use cocode_inference::LanguageModelCallOptions;
use cocode_inference::LanguageModelMessage;
use cocode_inference::StreamOptions;

/// Basic text generation: stream_request + collect.
pub async fn run_text(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text(
        "What is the capital of France? Reply with just the city name.",
    )]);

    let stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let response = stream.collect().await?;
    let text = response.text();

    assert!(!text.is_empty(), "Should return non-empty text");
    assert!(
        text.to_lowercase().contains("paris"),
        "Should mention Paris, got: {text}"
    );

    Ok(())
}

/// Non-streaming text generation.
pub async fn run_non_streaming(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text(
        "What is 2+3? Reply with just the number.",
    )]);

    let stream = client
        .stream_request(model, request, StreamOptions::non_streaming())
        .await?;

    let response = stream.collect().await?;
    let text = response.text();

    assert!(!text.is_empty(), "Should return non-empty text");
    assert!(text.contains('5'), "Should contain '5', got: {text}");

    Ok(())
}

/// Verify token usage is reported.
pub async fn run_token_usage(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request =
        LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text("Say hello.")]);

    let stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let response = stream.collect().await?;

    if let Some(usage) = &response.usage {
        assert!(
            usage.input_tokens > 0,
            "Input tokens should be > 0, got: {}",
            usage.input_tokens
        );
        assert!(
            usage.output_tokens > 0,
            "Output tokens should be > 0, got: {}",
            usage.output_tokens
        );
    }
    // Some providers may not report usage in streaming — don't fail

    Ok(())
}

/// Multi-turn conversation preserves context.
pub async fn run_multi_turn(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![
        LanguageModelMessage::user_text("My name is Alice."),
        LanguageModelMessage::assistant_text("Hello Alice! Nice to meet you."),
        LanguageModelMessage::user_text("What is my name? Reply with just the name."),
    ]);

    let stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let response = stream.collect().await?;
    let text = response.text().to_lowercase();

    assert!(
        text.contains("alice"),
        "Should remember the name 'Alice', got: {text}"
    );

    Ok(())
}
