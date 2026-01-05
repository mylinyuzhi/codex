//! Live integration tests for Z-AI adapter.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test -p codex-api --test live zai -- --test-threads=1
//! ```

use anyhow::Result;

use crate::common::adapter_config;
use crate::common::extract_function_calls;
use crate::common::extract_text;
use crate::common::has_function_call;
use crate::common::text_prompt;
use crate::common::tool_output_prompt;
use crate::common::tool_prompt;
use crate::common::weather_tool;
use crate::common::{self};
use crate::require_provider;

#[tokio::test]
async fn test_text_generation() -> Result<()> {
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
async fn test_tool_calling() -> Result<()> {
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

#[tokio::test]
async fn test_tool_call_complete_flow() -> Result<()> {
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
