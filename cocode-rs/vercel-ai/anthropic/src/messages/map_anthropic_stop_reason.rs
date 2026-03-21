use vercel_ai_provider::FinishReason;

/// Map an Anthropic stop reason to a unified `FinishReason`.
///
/// When `is_json_response_from_tool` is true, a `tool_use` stop reason
/// is mapped to `stop` instead of `tool_calls` (because the tool call
/// was used to produce a JSON structured output, not an actual tool invocation).
pub fn map_anthropic_stop_reason(
    reason: Option<&str>,
    is_json_response_from_tool: bool,
) -> FinishReason {
    let raw = reason.map(String::from);
    let unified = match reason {
        Some("end_turn" | "stop_sequence" | "pause_turn") => {
            vercel_ai_provider::UnifiedFinishReason::Stop
        }
        Some("refusal") => vercel_ai_provider::UnifiedFinishReason::ContentFilter,
        Some("tool_use") => {
            if is_json_response_from_tool {
                vercel_ai_provider::UnifiedFinishReason::Stop
            } else {
                vercel_ai_provider::UnifiedFinishReason::ToolCalls
            }
        }
        Some("max_tokens" | "model_context_window_exceeded") => {
            vercel_ai_provider::UnifiedFinishReason::Length
        }
        Some("compaction") => vercel_ai_provider::UnifiedFinishReason::Other,
        _ => vercel_ai_provider::UnifiedFinishReason::Other,
    };
    FinishReason { unified, raw }
}

#[cfg(test)]
#[path = "map_anthropic_stop_reason.test.rs"]
mod tests;
