//! Tool calling tests via ApiClient.

use anyhow::Result;
use cocode_inference::ApiClient;
use cocode_inference::LanguageModel;
use cocode_inference::LanguageModelCallOptions;
use cocode_inference::LanguageModelMessage;
use cocode_inference::LanguageModelToolChoice;
use cocode_inference::QueryResultType;
use cocode_inference::StreamOptions;

use crate::common::fixtures::weather_tool;

/// Non-streaming tool call: verify the model returns tool calls.
pub async fn run_tool_call(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text(
        "What's the weather in Tokyo?",
    )])
    .with_tools(vec![weather_tool()])
    .with_tool_choice(LanguageModelToolChoice::auto());

    let stream = client
        .stream_request(model, request, StreamOptions::non_streaming())
        .await?;

    let response = stream.collect().await?;

    assert!(
        response.has_tool_calls(),
        "Model should return tool calls for weather query"
    );

    // Verify tool call has the right name
    let tool_calls = response.tool_calls();
    assert!(!tool_calls.is_empty(), "Should have at least one tool call");

    let has_weather_call = tool_calls.iter().any(|tc| {
        if let cocode_inference::AssistantContentPart::ToolCall(call) = tc {
            call.tool_name == "get_weather"
        } else {
            false
        }
    });
    assert!(has_weather_call, "Should call 'get_weather' tool");

    Ok(())
}

/// Streaming tool call: verify tool call detection.
pub async fn run_tool_call_streaming(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text(
        "What's the weather in London?",
    )])
    .with_tools(vec![weather_tool()])
    .with_tool_choice(LanguageModelToolChoice::auto());

    let mut stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let mut has_tool_call = false;

    while let Some(result) = stream.next().await {
        let result = result?;
        if result.result_type == QueryResultType::Assistant && result.has_tool_calls() {
            has_tool_call = true;
        }
        if result.result_type == QueryResultType::Done {
            break;
        }
    }

    assert!(has_tool_call, "Streaming should yield tool call results");

    Ok(())
}
