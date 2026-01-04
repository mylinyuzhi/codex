//! Live integration tests for codex-api adapters.
//!
//! These tests run against real LLM provider APIs and require credentials
//! configured in `.env.test` or via environment variables.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all integration tests (all configured providers)
//! cargo test -p codex-api --ignored -- --test-threads=1
//!
//! # Run tests for a specific provider
//! cargo test -p codex-api --ignored genai -- --test-threads=1
//! cargo test -p codex-api --ignored anthropic -- --test-threads=1
//! cargo test -p codex-api --ignored openai -- --test-threads=1
//!
//! # Run a specific test category
//! cargo test -p codex-api --ignored tool_calling -- --test-threads=1
//! cargo test -p codex-api --ignored text_generation -- --test-threads=1
//! ```
//!
//! # Configuration
//!
//! Set environment variables for each provider:
//! - `CODEX_API_TEST_{PROVIDER}_API_KEY` - Required
//! - `CODEX_API_TEST_{PROVIDER}_MODEL` - Required
//! - `CODEX_API_TEST_{PROVIDER}_BASE_URL` - Optional
//!
//! Or use a `.env.test` file in the crate root.

mod common;

use anyhow::Result;
use common::TEST_RED_SQUARE_BASE64;
use common::adapter_config;
use common::assistant_message;
use common::extract_function_calls;
use common::extract_text;
use common::has_function_call;
use common::image_prompt;
use common::multi_turn_prompt;
use common::text_prompt;
use common::tool_output_prompt;
use common::tool_prompt;
use common::user_message;
use common::weather_tool;

// ============================================================================
// Genai (Google Gemini) Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_genai_text_generation() -> Result<()> {
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
async fn test_genai_tool_calling() -> Result<()> {
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
async fn test_genai_image_understanding() -> Result<()> {
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
async fn test_genai_reasoning_mode() -> Result<()> {
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

// ============================================================================
// Anthropic (Claude) Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_anthropic_text_generation() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
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
async fn test_anthropic_tool_calling() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
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
async fn test_anthropic_image_understanding() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
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
async fn test_anthropic_reasoning_mode() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
    let mut config = adapter_config(&cfg);

    // Enable extended thinking for Claude
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
    Ok(())
}

// ============================================================================
// OpenAI Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_openai_text_generation() -> Result<()> {
    let cfg = require_provider!("openai");

    // OpenAI uses built-in handling, not an adapter
    // Skip if openai adapter doesn't exist (uses native endpoint)
    if common::get_adapter("openai").is_none() {
        eprintln!("OpenAI uses native endpoint, skipping adapter test");
        return Ok(());
    }

    let adapter = common::get_adapter("openai").unwrap();
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
async fn test_openai_tool_calling() -> Result<()> {
    let cfg = require_provider!("openai");

    if common::get_adapter("openai").is_none() {
        eprintln!("OpenAI uses native endpoint, skipping adapter test");
        return Ok(());
    }

    let adapter = common::get_adapter("openai").unwrap();
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
async fn test_openai_image_understanding() -> Result<()> {
    let cfg = require_provider!("openai");

    if common::get_adapter("openai").is_none() {
        eprintln!("OpenAI uses native endpoint, skipping adapter test");
        return Ok(());
    }

    let adapter = common::get_adapter("openai").unwrap();
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

// ============================================================================
// Volcengine Ark Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_volc_ark_text_generation() -> Result<()> {
    let cfg = require_provider!("volc_ark");
    let adapter = common::get_adapter("volc_ark").expect("volc_ark adapter not found");
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
async fn test_volc_ark_tool_calling() -> Result<()> {
    let cfg = require_provider!("volc_ark");
    let adapter = common::get_adapter("volc_ark").expect("volc_ark adapter not found");
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

// ============================================================================
// Z-AI Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_zai_text_generation() -> Result<()> {
    let cfg = require_provider!("zai");
    let adapter = common::get_adapter("zai").expect("zai adapter not found");
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
async fn test_zai_tool_calling() -> Result<()> {
    let cfg = require_provider!("zai");
    let adapter = common::get_adapter("zai").expect("zai adapter not found");
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

// ============================================================================
// Token Usage Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_genai_token_usage() -> Result<()> {
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

#[tokio::test]
#[ignore]
async fn test_anthropic_token_usage() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
    let config = adapter_config(&cfg);

    let prompt = text_prompt("Say 'hello'.");
    let result = adapter.generate(&prompt, &config).await?;

    assert!(result.usage.is_some(), "Expected token usage in response");

    let usage = result.usage.unwrap();
    assert!(usage.input_tokens > 0, "Expected non-zero input tokens");
    assert!(usage.output_tokens > 0, "Expected non-zero output tokens");
    Ok(())
}

// ============================================================================
// Multi-Turn Conversation Tests
// ============================================================================

/// Test multi-turn conversation with context preservation.
///
/// Flow: User introduces name → Assistant acknowledges → User asks for name back
#[tokio::test]
#[ignore]
async fn test_genai_multi_turn_conversation() -> Result<()> {
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

#[tokio::test]
#[ignore]
async fn test_anthropic_multi_turn_conversation() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
    let config = adapter_config(&cfg);

    let history = vec![
        user_message("My name is Bob. Please remember it."),
        assistant_message("Hello Bob! I'll remember your name."),
    ];

    let prompt = multi_turn_prompt(history, "What is my name?", vec![]);
    let result = adapter.generate(&prompt, &config).await?;

    let text = extract_text(&result);
    assert!(
        text.to_lowercase().contains("bob"),
        "Expected 'bob' in response, got: {}",
        text
    );
    Ok(())
}

// ============================================================================
// Complete Tool Call Flow Tests
// ============================================================================

/// Test complete tool calling flow: question → tool call → tool output → final response.
///
/// This tests the full tool calling workflow, not just that a tool call is generated.
#[tokio::test]
#[ignore]
async fn test_genai_tool_call_complete_flow() -> Result<()> {
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

#[tokio::test]
#[ignore]
async fn test_anthropic_tool_call_complete_flow() -> Result<()> {
    let cfg = require_provider!("anthropic");
    let adapter = common::get_adapter("anthropic").expect("anthropic adapter not found");
    let config = adapter_config(&cfg);

    let question = "What's the weather in Tokyo?";
    let tools = vec![weather_tool()];

    // Step 1: Initial request
    let prompt1 = tool_prompt(question, tools.clone());
    let result1 = adapter.generate(&prompt1, &config).await?;

    let function_calls = extract_function_calls(&result1);
    assert!(
        !function_calls.is_empty(),
        "Expected at least one function call"
    );

    let call = &function_calls[0];
    assert_eq!(call.name, "get_weather");

    // Step 2: Provide tool output
    let tool_output = r#"{"temperature": "18°C", "condition": "cloudy"}"#;
    let prompt2 = tool_output_prompt(question, call, tool_output, tools);
    let result2 = adapter.generate(&prompt2, &config).await?;

    let text = extract_text(&result2);
    assert!(
        text.to_lowercase().contains("18") || text.to_lowercase().contains("cloudy"),
        "Expected response to include weather info, got: {}",
        text
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_volc_ark_tool_call_complete_flow() -> Result<()> {
    let cfg = require_provider!("volc_ark");
    let adapter = common::get_adapter("volc_ark").expect("volc_ark adapter not found");
    let config = adapter_config(&cfg);

    let question = "What's the weather in Beijing?";
    let tools = vec![weather_tool()];

    // Step 1: Initial request
    let prompt1 = tool_prompt(question, tools.clone());
    let result1 = adapter.generate(&prompt1, &config).await?;

    let function_calls = extract_function_calls(&result1);
    assert!(
        !function_calls.is_empty(),
        "Expected at least one function call"
    );

    let call = &function_calls[0];
    assert_eq!(call.name, "get_weather");

    // Step 2: Provide tool output
    let tool_output = r#"{"temperature": "25°C", "condition": "clear"}"#;
    let prompt2 = tool_output_prompt(question, call, tool_output, tools);
    let result2 = adapter.generate(&prompt2, &config).await?;

    let text = extract_text(&result2);
    assert!(
        text.contains("25") || text.to_lowercase().contains("clear"),
        "Expected response to include weather info, got: {}",
        text
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_zai_tool_call_complete_flow() -> Result<()> {
    let cfg = require_provider!("zai");
    let adapter = common::get_adapter("zai").expect("zai adapter not found");
    let config = adapter_config(&cfg);

    let question = "What's the weather in Shanghai?";
    let tools = vec![weather_tool()];

    // Step 1: Initial request
    let prompt1 = tool_prompt(question, tools.clone());
    let result1 = adapter.generate(&prompt1, &config).await?;

    let function_calls = extract_function_calls(&result1);
    assert!(
        !function_calls.is_empty(),
        "Expected at least one function call"
    );

    let call = &function_calls[0];
    assert_eq!(call.name, "get_weather");

    // Step 2: Provide tool output
    let tool_output = r#"{"temperature": "28°C", "condition": "humid"}"#;
    let prompt2 = tool_output_prompt(question, call, tool_output, tools);
    let result2 = adapter.generate(&prompt2, &config).await?;

    let text = extract_text(&result2);
    assert!(
        text.contains("28") || text.to_lowercase().contains("humid"),
        "Expected response to include weather info, got: {}",
        text
    );
    Ok(())
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_list_configured_providers() {
    let providers = common::config::list_configured_providers();
    eprintln!("Configured providers: {:?}", providers);
    // This test just verifies the function doesn't panic
}

#[test]
fn test_all_builtin_adapters_available() {
    // Verify all built-in adapters are registered
    assert!(common::get_adapter("genai").is_some());
    assert!(common::get_adapter("anthropic").is_some());
    assert!(common::get_adapter("volc_ark").is_some());
    assert!(common::get_adapter("zai").is_some());
}
