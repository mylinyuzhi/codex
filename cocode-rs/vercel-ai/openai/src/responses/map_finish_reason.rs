use vercel_ai_provider::FinishReason;
use vercel_ai_provider::UnifiedFinishReason;

/// Map an OpenAI Responses API finish reason to an SDK `FinishReason`.
///
/// The Responses API uses different reason strings than Chat:
/// - `"max_output_tokens"` instead of `"length"`
/// - `null` with function calls means `tool-calls`
pub fn map_openai_responses_finish_reason(
    finish_reason: Option<&str>,
    has_function_call: bool,
) -> FinishReason {
    let raw = finish_reason.map(String::from);
    let unified = match finish_reason {
        None if has_function_call => UnifiedFinishReason::ToolCalls,
        None => UnifiedFinishReason::Stop,
        Some("max_output_tokens") => UnifiedFinishReason::Length,
        Some("content_filter") => UnifiedFinishReason::ContentFilter,
        _ if has_function_call => UnifiedFinishReason::ToolCalls,
        _ => UnifiedFinishReason::Other,
    };
    FinishReason { unified, raw }
}

#[cfg(test)]
#[path = "map_finish_reason.test.rs"]
mod tests;
