//! Cross-provider conversation tests.
//!
//! Tests that verify messages from one provider can be correctly
//! used with another provider via the vercel-ai unified message format.

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use vercel_ai::GenerateTextOptions;
use vercel_ai::LanguageModel;
use vercel_ai::LanguageModelV4;
use vercel_ai::Prompt;
use vercel_ai::StreamTextOptions;
use vercel_ai::TextStreamPart;
use vercel_ai::generate_text;
use vercel_ai::stream_text;
use vercel_ai_provider::LanguageModelV4Message;

/// Run cross-provider conversation test.
///
/// Gets a response from the source provider, builds a multi-turn prompt
/// including that response, and sends it to the target provider.
pub async fn run(
    source_model: &Arc<dyn LanguageModelV4>,
    target_model: &Arc<dyn LanguageModelV4>,
) -> Result<()> {
    // Step 1: Get a response from the source provider
    let source_result = generate_text(GenerateTextOptions::new(
        LanguageModel::from_v4(source_model.clone()),
        Prompt::user("What is 2+2? Reply with just the number.")
            .with_system("You are a helpful assistant. Be concise."),
    ))
    .await?;

    // Step 2: Create follow-up request with history for target provider
    let target_result = generate_text(GenerateTextOptions::new(
        LanguageModel::from_v4(target_model.clone()),
        Prompt::messages(vec![
            LanguageModelV4Message::system("You are a helpful assistant. Be concise."),
            LanguageModelV4Message::user_text("What is 2+2? Reply with just the number."),
            LanguageModelV4Message::assistant_text(&source_result.text),
            LanguageModelV4Message::user_text(
                "What is double that number? Reply with just the number.",
            ),
        ]),
    ))
    .await?;

    // Verify we got a valid response containing "8"
    let response_text = target_result.text.trim();
    assert!(
        !response_text.is_empty(),
        "Target provider should return a response"
    );
    assert!(
        response_text.contains('8'),
        "Response should contain '8', got: {response_text}"
    );

    Ok(())
}

/// Run streaming cross-provider test.
///
/// Tests that streaming works correctly with cross-provider message history.
/// Gets a response from the source provider, then streams a follow-up on the
/// target provider using that response as conversation history.
pub async fn run_streaming(
    source_model: &Arc<dyn LanguageModelV4>,
    target_model: &Arc<dyn LanguageModelV4>,
) -> Result<()> {
    // Step 1: Get a response from source provider
    let source_result = generate_text(GenerateTextOptions::new(
        LanguageModel::from_v4(source_model.clone()),
        Prompt::user("What is the capital of France?")
            .with_system("You are a helpful assistant. Be concise."),
    ))
    .await?;

    // Step 2: Stream follow-up on target with source's response as history
    let result = stream_text(StreamTextOptions::new(
        LanguageModel::from_v4(target_model.clone()),
        Prompt::messages(vec![
            LanguageModelV4Message::system("You are a helpful assistant."),
            LanguageModelV4Message::user_text("What is the capital of France?"),
            LanguageModelV4Message::assistant_text(&source_result.text),
            LanguageModelV4Message::user_text("What is a famous landmark there?"),
        ]),
    ));

    // Consume the stream
    let mut stream = result.stream;
    let mut collected_text = String::new();

    while let Some(part) = stream.next().await {
        if let TextStreamPart::TextDelta { delta, .. } = part {
            collected_text.push_str(&delta);
        }
    }

    // Verify response mentions a Paris landmark
    let text = collected_text.to_lowercase();
    assert!(!text.is_empty(), "Should get a streaming response");
    let mentions_landmark = text.contains("eiffel")
        || text.contains("louvre")
        || text.contains("notre")
        || text.contains("arc")
        || text.contains("tower")
        || text.contains("museum");
    assert!(
        mentions_landmark || text.len() > 20,
        "Response should mention a Paris landmark or be substantive: {text}"
    );

    Ok(())
}
