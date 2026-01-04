//! Live integration tests for Google Gemini (genai) adapter.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test -p codex-api --ignored live::genai -- --test-threads=1
//! ```

use anyhow::Result;

use crate::common::TEST_RED_SQUARE_BASE64;
use crate::common::adapter_config;
use crate::common::assistant_message;
use crate::common::extract_function_calls;
use crate::common::extract_text;
use crate::common::has_function_call;
use crate::common::image_prompt;
use crate::common::multi_turn_prompt;
use crate::common::text_prompt;
use crate::common::tool_output_prompt;
use crate::common::tool_prompt;
use crate::common::user_message;
use crate::common::weather_tool;
use crate::common::{self};
use crate::require_provider;

#[tokio::test]
#[ignore]
async fn test_text_generation() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    let prompt = text_prompt("Say 'hello' in exactly one word, nothing else.");
    let result = adapter.generate(&prompt, &config).await?;

    let text = extract_text(&result);
    assert!(
        text.to_lowercase().contains("hello"),
        "Expected 'hello' in response, got: {}",
        text
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_tool_calling() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    let prompt = tool_prompt(
        "What's the weather in Tokyo? Use the get_weather tool.",
        vec![weather_tool()],
    );
    let result = adapter.generate(&prompt, &config).await?;

    assert!(
        has_function_call(&result, "get_weather"),
        "Expected get_weather function call in response"
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_image_understanding() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    let prompt = image_prompt(
        "What color is this square? Answer with just the color name.",
        TEST_RED_SQUARE_BASE64,
    );
    let result = adapter.generate(&prompt, &config).await?;

    let text = extract_text(&result);
    assert!(
        text.to_lowercase().contains("red"),
        "Expected 'red' in response, got: {}",
        text
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_reasoning_mode() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let mut config = adapter_config(&cfg);

    // Enable thinking mode via extra config
    config.extra = Some(serde_json::json!({
        "thinking": {
            "type": "enabled",
            "budget_tokens": 1024
        }
    }));

    let prompt = text_prompt("What is 17 * 23? Think step by step.");
    let result = adapter.generate(&prompt, &config).await?;

    let text = extract_text(&result);
    assert!(
        text.contains("391"),
        "Expected '391' in response, got: {}",
        text
    );
    // Note: Reasoning items may or may not be returned depending on model
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_token_usage() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    let prompt = text_prompt("Say 'hello'.");
    let result = adapter.generate(&prompt, &config).await?;

    assert!(result.usage.is_some(), "Expected token usage in response");

    let usage = result.usage.unwrap();
    assert!(usage.input_tokens > 0, "Expected non-zero input tokens");
    assert!(usage.output_tokens > 0, "Expected non-zero output tokens");
    Ok(())
}

/// Test multi-turn conversation with context preservation.
///
/// Flow: User introduces name -> Assistant acknowledges -> User asks for name back
#[tokio::test]
#[ignore]
async fn test_multi_turn_conversation() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    // Build conversation history
    let history = vec![
        user_message("My name is Alice. Please remember it."),
        assistant_message("Hello Alice! I'll remember your name."),
    ];

    // Second turn - ask for name back
    let prompt = multi_turn_prompt(history, "What is my name?", vec![]);
    let result = adapter.generate(&prompt, &config).await?;

    let text = extract_text(&result);
    assert!(
        text.to_lowercase().contains("alice"),
        "Expected 'alice' in response (context should be preserved), got: {}",
        text
    );
    Ok(())
}

/// Test complete tool calling flow: question -> tool call -> tool output -> final response.
///
/// This tests the full tool calling workflow, not just that a tool call is generated.
#[tokio::test]
#[ignore]
async fn test_tool_call_complete_flow() -> Result<()> {
    let cfg = require_provider!("genai");
    let adapter = common::get_adapter("genai").expect("genai adapter not found");
    let config = adapter_config(&cfg);

    let question = "What's the weather in Tokyo?";
    let tools = vec![weather_tool()];

    // Step 1: Initial request - should get a tool call
    let prompt1 = tool_prompt(question, tools.clone());
    let result1 = adapter.generate(&prompt1, &config).await?;

    let function_calls = extract_function_calls(&result1);
    assert!(
        !function_calls.is_empty(),
        "Expected at least one function call"
    );

    let call = &function_calls[0];
    assert_eq!(call.name, "get_weather", "Expected get_weather call");

    // Step 2: Provide tool output and get final response
    let tool_output = r#"{"temperature": "22°C", "condition": "sunny", "humidity": "45%"}"#;
    let prompt2 = tool_output_prompt(question, call, tool_output, tools);
    let result2 = adapter.generate(&prompt2, &config).await?;

    let text = extract_text(&result2);
    assert!(
        text.to_lowercase().contains("22") || text.to_lowercase().contains("sunny"),
        "Expected response to include weather info from tool output, got: {}",
        text
    );
    Ok(())
}
