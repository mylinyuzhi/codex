//! StreamProcessor integration tests.
//!
//! Tests the mid-level StreamProcessor API against real LLM providers.
//! Verifies that streaming responses are correctly accumulated into snapshots.

use std::sync::Arc;

use anyhow::Result;
use vercel_ai::LanguageModelV4;
use vercel_ai::LanguageModelV4CallOptions;
use vercel_ai::LanguageModelV4Tool;
use vercel_ai::StreamProcessor;
use vercel_ai::prepare_tool_definitions;
use vercel_ai_provider::LanguageModelV4Message;

use crate::common::weather_tool_registry_no_exec;

/// Build a simple text prompt as call options.
fn text_prompt(system: &str, user: &str) -> LanguageModelV4CallOptions {
    LanguageModelV4CallOptions {
        prompt: vec![
            LanguageModelV4Message::system(system),
            LanguageModelV4Message::user_text(user),
        ],
        ..Default::default()
    }
}

/// Test StreamProcessor.collect() produces a complete snapshot with text.
pub async fn run_collect(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let options = text_prompt(
        "You are a helpful assistant. Be concise.",
        "Say 'hello world' exactly.",
    );

    let stream_result = model.do_stream(options).await?;
    let snapshot = StreamProcessor::new(stream_result).collect().await?;

    assert!(snapshot.is_complete, "Expected stream to complete");
    assert!(
        snapshot.finish_reason.is_some(),
        "Expected a finish reason in snapshot"
    );
    assert!(
        snapshot.usage.is_some(),
        "Expected usage data in completed snapshot"
    );
    assert!(
        snapshot.text.to_lowercase().contains("hello"),
        "Expected 'hello' in collected text, got: {}",
        snapshot.text
    );

    Ok(())
}

/// Test StreamProcessor.into_text() convenience method.
pub async fn run_into_text(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let options = text_prompt(
        "You are a helpful assistant. Be concise.",
        "Say the word 'pineapple' and nothing else.",
    );

    let stream_result = model.do_stream(options).await?;
    let text = StreamProcessor::new(stream_result).into_text().await?;

    assert!(
        text.to_lowercase().contains("pineapple"),
        "Expected 'pineapple' in text, got: {text}"
    );

    Ok(())
}

/// Test StreamProcessor.next() yields incremental snapshots.
pub async fn run_next_incremental(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let options = text_prompt(
        "You are a helpful assistant. Be concise.",
        "Count from 1 to 5, separated by spaces.",
    );

    let stream_result = model.do_stream(options).await?;
    let mut processor = StreamProcessor::new(stream_result);

    let mut event_count = 0;
    let mut last_text_len = 0;
    let mut saw_text_grow = false;

    while let Some(result) = processor.next().await {
        let (_part, snapshot) = result?;
        event_count += 1;

        if snapshot.text.len() > last_text_len {
            saw_text_grow = true;
        }
        last_text_len = snapshot.text.len();
    }

    assert!(event_count > 1, "Expected multiple stream events");
    assert!(
        saw_text_grow,
        "Expected text to grow incrementally during streaming"
    );
    assert!(last_text_len > 0, "Expected non-empty final text");

    Ok(())
}

/// Test StreamProcessor with token usage reporting.
pub async fn run_usage(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let options = text_prompt("You are a helpful assistant. Be concise.", "Say 'test'.");

    let stream_result = model.do_stream(options).await?;
    let snapshot = StreamProcessor::new(stream_result).collect().await?;

    assert!(snapshot.is_complete);
    let usage = snapshot
        .usage
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Expected usage in completed snapshot"))?;
    assert!(
        usage.total_input_tokens() > 0,
        "Expected non-zero input tokens"
    );
    assert!(
        usage.total_output_tokens() > 0,
        "Expected non-zero output tokens"
    );

    Ok(())
}

/// Test StreamProcessor accumulates tool calls from streaming.
pub async fn run_tool_calls(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let registry = weather_tool_registry_no_exec();
    let tool_defs: Vec<LanguageModelV4Tool> = prepare_tool_definitions(&registry)
        .into_iter()
        .map(LanguageModelV4Tool::Function)
        .collect();

    let options = LanguageModelV4CallOptions {
        prompt: vec![
            LanguageModelV4Message::system(
                "You are a helpful assistant. Use the provided tools when appropriate.",
            ),
            LanguageModelV4Message::user_text(
                "What's the weather in Tokyo? Use the get_weather tool.",
            ),
        ],
        tools: Some(tool_defs),
        ..Default::default()
    };

    let stream_result = model.do_stream(options).await?;
    let snapshot = StreamProcessor::new(stream_result).collect().await?;

    assert!(snapshot.is_complete);
    assert!(
        snapshot.has_tool_calls(),
        "Expected tool calls in snapshot, got none"
    );

    let completed = snapshot.completed_tool_calls();
    assert!(
        !completed.is_empty(),
        "Expected at least one completed tool call"
    );
    assert_eq!(
        completed[0].tool_name, "get_weather",
        "Expected get_weather tool call, got: {}",
        completed[0].tool_name
    );
    assert!(
        !completed[0].input_json.is_empty(),
        "Expected non-empty tool input JSON"
    );

    Ok(())
}

/// Test StreamProcessor with multi-turn conversation context.
pub async fn run_multi_turn(model: &Arc<dyn LanguageModelV4>) -> Result<()> {
    let options = LanguageModelV4CallOptions {
        prompt: vec![
            LanguageModelV4Message::system("You are a helpful assistant."),
            LanguageModelV4Message::user_text("My name is StreamTestUser. Please remember it."),
            LanguageModelV4Message::assistant_text(
                "Hello StreamTestUser! I'll remember your name.",
            ),
            LanguageModelV4Message::user_text("What is my name?"),
        ],
        ..Default::default()
    };

    let stream_result = model.do_stream(options).await?;
    let text = StreamProcessor::new(stream_result).into_text().await?;

    assert!(
        text.to_lowercase().contains("streamtestuser"),
        "Expected 'streamtestuser' in response, got: {text}"
    );

    Ok(())
}
