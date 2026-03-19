use super::*;
use crate::prompt::CallSettings;
use std::collections::HashMap;
use vercel_ai_provider::language_model::v4::LanguageModelV4FunctionTool;

#[test]
fn test_build_call_options_defaults() {
    let settings = CallSettings::default();
    let opts = build_call_options(&settings, &None, &None, &None, &None, vec![], &None);

    assert!(opts.max_output_tokens.is_none());
    assert!(opts.temperature.is_none());
    assert!(opts.tools.is_none());
}

#[test]
fn test_build_call_options_all_settings() {
    let mut headers = HashMap::new();
    headers.insert("X-Custom".to_string(), "value".to_string());

    let settings = CallSettings::new()
        .with_max_tokens(1000)
        .with_temperature(0.7)
        .with_top_p(0.9)
        .with_top_k(40)
        .with_stop_sequences(vec!["STOP".to_string()])
        .with_frequency_penalty(0.5)
        .with_presence_penalty(0.3)
        .with_seed(42)
        .with_headers(headers);

    let opts = build_call_options(&settings, &None, &None, &None, &None, vec![], &None);

    assert_eq!(opts.max_output_tokens, Some(1000));
    assert_eq!(opts.temperature, Some(0.7));
    assert_eq!(opts.top_p, Some(0.9));
    assert_eq!(opts.top_k, Some(40));
    assert_eq!(opts.stop_sequences, Some(vec!["STOP".to_string()]));
    assert_eq!(opts.frequency_penalty, Some(0.5));
    assert_eq!(opts.presence_penalty, Some(0.3));
    assert_eq!(opts.seed, Some(42));
    assert!(opts.headers.is_some());
}

#[test]
fn test_build_call_options_with_abort_signal() {
    let settings = CallSettings::default();
    let signal = CancellationToken::new();
    let opts = build_call_options(&settings, &None, &Some(signal), &None, &None, vec![], &None);

    assert!(opts.abort_signal.is_some());
}

#[test]
fn test_filter_active_tools_none() {
    assert!(filter_active_tools(&None, &None).is_none());
}

#[test]
fn test_filter_active_tools_no_filter() {
    let tools = vec![LanguageModelV4Tool::function(
        LanguageModelV4FunctionTool::new("tool_a", serde_json::json!({})),
    )];
    let result = filter_active_tools(&Some(tools), &None);
    assert_eq!(result.as_ref().map(Vec::len), Some(1));
}

#[test]
fn test_filter_active_tools_with_filter() {
    let tools = vec![
        LanguageModelV4Tool::function(LanguageModelV4FunctionTool::new(
            "tool_a",
            serde_json::json!({}),
        )),
        LanguageModelV4Tool::function(LanguageModelV4FunctionTool::new(
            "tool_b",
            serde_json::json!({}),
        )),
    ];
    let active = vec!["tool_a".to_string()];
    let result = filter_active_tools(&Some(tools), &Some(active));
    assert_eq!(result.as_ref().map(Vec::len), Some(1));
}
