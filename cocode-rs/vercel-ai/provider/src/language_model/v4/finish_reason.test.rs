use super::*;

#[test]
fn test_unified_finish_reason_default() {
    let reason = UnifiedFinishReason::default();
    assert!(reason.is_stop());
}

#[test]
fn test_unified_finish_reason_is_stop() {
    assert!(UnifiedFinishReason::Stop.is_stop());
    assert!(!UnifiedFinishReason::Length.is_stop());
    assert!(!UnifiedFinishReason::ToolCalls.is_stop());
}

#[test]
fn test_unified_finish_reason_is_length() {
    assert!(UnifiedFinishReason::Length.is_length());
    assert!(!UnifiedFinishReason::Stop.is_length());
    assert!(!UnifiedFinishReason::Error.is_length());
}

#[test]
fn test_unified_finish_reason_is_content_filter() {
    assert!(UnifiedFinishReason::ContentFilter.is_content_filter());
    assert!(!UnifiedFinishReason::Stop.is_content_filter());
    assert!(!UnifiedFinishReason::Error.is_content_filter());
}

#[test]
fn test_unified_finish_reason_is_tool_calls() {
    assert!(UnifiedFinishReason::ToolCalls.is_tool_calls());
    assert!(!UnifiedFinishReason::Stop.is_tool_calls());
    assert!(!UnifiedFinishReason::Length.is_tool_calls());
}

#[test]
fn test_unified_finish_reason_is_error() {
    assert!(UnifiedFinishReason::Error.is_error());
    assert!(!UnifiedFinishReason::Stop.is_error());
    assert!(!UnifiedFinishReason::ToolCalls.is_error());
}

#[test]
fn test_unified_finish_reason_display() {
    assert_eq!(format!("{}", UnifiedFinishReason::Stop), "stop");
    assert_eq!(format!("{}", UnifiedFinishReason::Length), "length");
    assert_eq!(
        format!("{}", UnifiedFinishReason::ContentFilter),
        "content-filter"
    );
    assert_eq!(format!("{}", UnifiedFinishReason::ToolCalls), "tool-calls");
    assert_eq!(format!("{}", UnifiedFinishReason::Error), "error");
    assert_eq!(format!("{}", UnifiedFinishReason::Other), "other");
}

#[test]
fn test_unified_finish_reason_serde() {
    let reason = UnifiedFinishReason::Stop;
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, r#""stop""#);

    let parsed: UnifiedFinishReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, UnifiedFinishReason::Stop);
}

#[test]
fn test_finish_reason_default() {
    let reason = FinishReason::default();
    assert!(reason.is_stop());
    assert!(reason.raw.is_none());
}

#[test]
fn test_finish_reason_stop() {
    let reason = FinishReason::stop();
    assert!(reason.is_stop());
    assert!(reason.raw.is_none());
}

#[test]
fn test_finish_reason_length() {
    let reason = FinishReason::length();
    assert!(reason.is_length());
}

#[test]
fn test_finish_reason_content_filter() {
    let reason = FinishReason::content_filter();
    assert!(reason.is_content_filter());
}

#[test]
fn test_finish_reason_tool_calls() {
    let reason = FinishReason::tool_calls();
    assert!(reason.is_tool_calls());
}

#[test]
fn test_finish_reason_error() {
    let reason = FinishReason::error();
    assert!(reason.is_error());
}

#[test]
fn test_finish_reason_with_raw() {
    let reason = FinishReason::with_raw(UnifiedFinishReason::Stop, "complete");
    assert!(reason.is_stop());
    assert_eq!(reason.raw, Some("complete".to_string()));
}

#[test]
fn test_finish_reason_with_raw_value() {
    let reason = FinishReason::stop().with_raw_value("end_turn");
    assert!(reason.is_stop());
    assert_eq!(reason.raw, Some("end_turn".to_string()));
}

#[test]
fn test_finish_reason_from_unified() {
    let reason: FinishReason = UnifiedFinishReason::ToolCalls.into();
    assert!(reason.is_tool_calls());
    assert!(reason.raw.is_none());
}

#[test]
fn test_finish_reason_serde() {
    let reason = FinishReason::stop();
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, r#"{"unified":"stop"}"#);

    let parsed: FinishReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.unified, UnifiedFinishReason::Stop);
    assert!(parsed.raw.is_none());
}

#[test]
fn test_finish_reason_with_raw_serde() {
    let reason = FinishReason::with_raw(UnifiedFinishReason::Stop, "end_turn");
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, r#"{"unified":"stop","raw":"end_turn"}"#);

    let parsed: FinishReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.unified, UnifiedFinishReason::Stop);
    assert_eq!(parsed.raw, Some("end_turn".to_string()));
}
