//! Reusable test fixtures: prompts and tool definitions.
//!
//! Tools are surfaced as `LanguageModelTool` (provider-protocol shape via
//! the `coco_inference` seam) rather than executable `vercel-ai` `Tool`s.
//! The SDK suite calls `ApiClient.query` / `query_stream` directly and
//! only inspects whether the model emits a tool call — actual tool
//! execution is covered end-to-end by the `cli_deepseek` suite.

use coco_inference::LanguageModelFunctionTool;
use coco_inference::LanguageModelTool;
use coco_inference::QueryResult;
use coco_llm_types::AssistantContentPart;

/// `LanguageModelTool::Function` definition for a one-arg `get_weather`
/// tool. Consumers feed this into `QueryParams.tools`.
pub fn weather_tool_def() -> LanguageModelTool {
    LanguageModelTool::Function(LanguageModelFunctionTool {
        name: "get_weather".into(),
        description: Some("Get the current weather for a city.".into()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "description": "The city name" }
            },
            "required": ["city"]
        }),
        input_examples: None,
        strict: None,
        provider_options: None,
    })
}

/// `true` when the assistant content contains a tool call with the given
/// function name. Mirrors `vercel-ai`'s `has_tool_call_named` but works
/// off `QueryResult.content` (the seam-aliased shape).
pub fn has_tool_call_named(result: &QueryResult, name: &str) -> bool {
    result.content.iter().any(|part| match part {
        AssistantContentPart::ToolCall(tc) => tc.tool_name == name,
        _ => false,
    })
}

/// Concatenate every `Text` content part — simulates what
/// `vercel_ai::generate_text` returns as `result.text`.
pub fn extract_text(result: &QueryResult) -> String {
    let mut out = String::new();
    for part in &result.content {
        if let AssistantContentPart::Text(t) = part {
            out.push_str(&t.text);
        }
    }
    out
}
