//! Vision/image understanding tests.
//!
//! Tests multi-modal image understanding capabilities via the vercel-ai
//! unified message format.

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use vercel_ai::GenerateTextOptions;
use vercel_ai::LanguageModel;
use vercel_ai::LanguageModelV4;
use vercel_ai::Prompt;
use vercel_ai::UserContentPart;
use vercel_ai::generate_text;
use vercel_ai_provider::LanguageModelV4Message;

use crate::common::TEST_RED_SQUARE_BASE64;

/// Test image understanding.
///
/// Sends a red square image and asks the model to identify its color.
pub async fn run(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    // Parse data URL: "data:image/png;base64,<data>"
    let base64_data = TEST_RED_SQUARE_BASE64
        .strip_prefix("data:image/png;base64,")
        .context("fixture should be a PNG data URL")?;
    let image_bytes = STANDARD.decode(base64_data)?;

    let result = generate_text(GenerateTextOptions::new(
        LanguageModel::from_v4(model.clone()),
        Prompt::messages(vec![LanguageModelV4Message::user(vec![
            UserContentPart::image(image_bytes, "image/png"),
            UserContentPart::text(
                "What color is this square? Answer with just the color name.",
            ),
        ])])
        .with_system("You are a helpful assistant. Be concise."),
    ))
    .await?;

    let text = result.text.to_lowercase();
    assert!(
        text.contains("red"),
        "Expected 'red' in response, got: {text}"
    );

    Ok(())
}
