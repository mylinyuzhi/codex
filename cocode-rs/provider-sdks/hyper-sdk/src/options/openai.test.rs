use super::*;
use crate::options::downcast_options;

#[test]
fn test_openai_options() {
    let opts = OpenAIOptions::new()
        .with_reasoning_effort(ReasoningEffort::High)
        .with_previous_response_id("resp_123");

    assert_eq!(opts.reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(opts.previous_response_id, Some("resp_123".to_string()));
}

#[test]
fn test_downcast() {
    let opts: Box<dyn ProviderOptionsData> = OpenAIOptions::new()
        .with_reasoning_effort(ReasoningEffort::Low)
        .boxed();

    let openai_opts = downcast_options::<OpenAIOptions>(&opts);
    assert!(openai_opts.is_some());
    assert_eq!(
        openai_opts.unwrap().reasoning_effort,
        Some(ReasoningEffort::Low)
    );
}

#[test]
fn test_reasoning_summary() {
    let opts = OpenAIOptions::new()
        .with_reasoning_summary(ReasoningSummary::Detailed)
        .with_include_encrypted_content(true);

    assert_eq!(opts.reasoning_summary, Some(ReasoningSummary::Detailed));
    assert_eq!(opts.include_encrypted_content, Some(true));
}

#[test]
fn test_reasoning_summary_serde() {
    let summary = ReasoningSummary::Concise;
    let json = serde_json::to_string(&summary).unwrap();
    assert_eq!(json, "\"concise\"");

    let parsed: ReasoningSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ReasoningSummary::Concise);
}
