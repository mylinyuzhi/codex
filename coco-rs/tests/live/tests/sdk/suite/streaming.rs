//! Streaming tests via `coco_inference::ApiClient::query_stream`.

use anyhow::Result;
use coco_inference::LanguageModelMessage;
use coco_inference::QueryParams;
use coco_inference::StreamEvent;

use crate::common::LiveTarget;
use crate::common::usage_report;
use crate::common::weather_tool_def;

/// Asserts: at least one event arrived, a `Finish` was emitted, and the
/// concatenated text contains `hello`.
pub async fn run(target: &LiveTarget) -> Result<()> {
    let params = QueryParams {
        prompt: vec![
            LanguageModelMessage::system("You are a helpful assistant. Be concise."),
            LanguageModelMessage::user_text("Say 'hello world' exactly."),
        ],
        // 1024 leaves headroom for reasoning models — see the same
        // rationale on `basic::params_for`.
        max_tokens: Some(1024),
        thinking_level: None,
        fast_mode: false,
        tools: None,
        context_management: None,
        query_source: Some("coco-tests-live::sdk::streaming::run".into()),
        agent_id: None,
        time_since_last_assistant_ms: None,
        agentic: false,
        cache: None,
    };

    let mut rx = target.client.query_stream(&params).await?;
    let mut text = String::new();
    let mut events = 0usize;
    let mut saw_finish = false;
    let mut final_usage = coco_types::TokenUsage::default();

    while let Some(event) = rx.recv().await {
        events += 1;
        match event {
            StreamEvent::TextDelta { text: delta } => text.push_str(&delta),
            StreamEvent::Finish { usage, .. } => {
                saw_finish = true;
                final_usage = usage;
            }
            _ => {}
        }
    }
    usage_report::record(
        target.provider,
        &target.model,
        "streaming.run",
        &final_usage,
    );

    assert!(
        events > 0,
        "{}/{}: stream produced no events",
        target.provider,
        target.model
    );
    assert!(
        saw_finish,
        "{}/{}: stream did not emit Finish event",
        target.provider, target.model
    );
    assert!(
        text.to_lowercase().contains("hello"),
        "{}/{}: streamed text missing 'hello': {text}",
        target.provider,
        target.model
    );
    Ok(())
}

/// Streaming + tool-calling. Asserts a `ToolCallStart` event for `get_weather`.
pub async fn run_with_tools(target: &LiveTarget) -> Result<()> {
    let params = QueryParams {
        prompt: vec![
            LanguageModelMessage::system("You are a helpful assistant. Use the provided tools."),
            LanguageModelMessage::user_text(
                "What's the weather in Tokyo? Use the get_weather tool.",
            ),
        ],
        max_tokens: Some(256),
        thinking_level: None,
        fast_mode: false,
        tools: Some(vec![weather_tool_def()]),
        context_management: None,
        query_source: Some("coco-tests-live::sdk::streaming::run_with_tools".into()),
        agent_id: None,
        time_since_last_assistant_ms: None,
        agentic: false,
        cache: None,
    };

    let mut rx = target.client.query_stream(&params).await?;
    let mut tool_name = String::new();
    let mut saw_tool_call_start = false;
    let mut final_usage = coco_types::TokenUsage::default();

    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::ToolCallStart {
                tool_name: name, ..
            } => {
                saw_tool_call_start = true;
                tool_name = name;
            }
            StreamEvent::Finish { usage, .. } => final_usage = usage,
            _ => {}
        }
    }
    usage_report::record(
        target.provider,
        &target.model,
        "streaming.with_tools",
        &final_usage,
    );

    assert!(
        saw_tool_call_start,
        "{}/{}: stream did not emit a ToolCallStart event",
        target.provider, target.model
    );
    assert_eq!(
        tool_name, "get_weather",
        "{}/{}: unexpected tool name: {tool_name}",
        target.provider, target.model
    );
    Ok(())
}
