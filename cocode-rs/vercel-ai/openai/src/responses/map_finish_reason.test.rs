use super::*;

#[test]
fn none_no_fn_call_is_stop() {
    let r = map_openai_responses_finish_reason(None, false);
    assert_eq!(r.unified, UnifiedFinishReason::Stop);
}

#[test]
fn none_with_fn_call_is_tool_calls() {
    let r = map_openai_responses_finish_reason(None, true);
    assert_eq!(r.unified, UnifiedFinishReason::ToolCalls);
}

#[test]
fn max_output_tokens_is_length() {
    let r = map_openai_responses_finish_reason(Some("max_output_tokens"), false);
    assert_eq!(r.unified, UnifiedFinishReason::Length);
}

#[test]
fn content_filter() {
    let r = map_openai_responses_finish_reason(Some("content_filter"), false);
    assert_eq!(r.unified, UnifiedFinishReason::ContentFilter);
}
