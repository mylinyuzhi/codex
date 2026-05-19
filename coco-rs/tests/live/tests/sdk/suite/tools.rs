//! Tool-calling tests via `coco_inference::ApiClient::query`.
//!
//! Validates the model emits a `get_weather` tool call. Full multi-step
//! tool-execution flow (call → execute → respond) is not covered here:
//! it requires an executable tool runtime, which lives in
//! `coco-tool-runtime` and is exercised end-to-end by the
//! `cli_deepseek` suite.

use anyhow::Result;
use coco_inference::QueryParams;
use coco_llm_types::LlmMessage;

use crate::common::LiveTarget;
use crate::common::has_tool_call_named;
use crate::common::usage_report;
use crate::common::weather_tool_def;

/// Asserts the model emits a `get_weather` tool call when prompted.
pub async fn run(target: &LiveTarget) -> Result<()> {
    let params = QueryParams {
        // Same shape + rationale as `streaming::run_with_tools`:
        // 4096-token budget + imperative system prompt to keep
        // Gemini-3 reliably emitting the tool call.
        prompt: vec![
            LlmMessage::system(
                "You are a helpful assistant. For weather questions you MUST call \
                 the get_weather tool — do not answer with prose, do not refuse, \
                 do not return an empty message.",
            ),
            LlmMessage::user_text(
                "What's the weather in Tokyo? Call get_weather with city='Tokyo'.",
            ),
        ],
        max_tokens: Some(4096),
        thinking_level: None,
        fast_mode: false,
        tools: Some(vec![weather_tool_def()]),
        context_management: None,
        query_source: Some("coco-tests-live::sdk::tools::run".into()),
        agent_id: None,
        time_since_last_assistant_ms: None,
        agentic: false,
        cache: None,
        stop_sequences: None,
    };
    let result = target.client.query(&params).await?;
    usage_report::record(target.provider, &target.model, "tools.run", &result.usage);

    assert!(
        has_tool_call_named(&result, "get_weather"),
        "{}/{}: no get_weather tool call in response (content count={})",
        target.provider,
        target.model,
        result.content.len()
    );
    Ok(())
}
