use super::*;
use crate::options::downcast_options;

#[test]
fn test_volcengine_options() {
    let opts = VolcengineOptions::new()
        .with_thinking_budget(2048)
        .with_previous_response_id("resp_123")
        .with_caching(true)
        .with_reasoning_effort(ReasoningEffort::High);

    assert_eq!(opts.thinking_budget_tokens, Some(2048));
    assert_eq!(opts.previous_response_id, Some("resp_123".to_string()));
    assert_eq!(opts.caching_enabled, Some(true));
    assert_eq!(opts.reasoning_effort, Some(ReasoningEffort::High));
}

#[test]
fn test_downcast() {
    let opts: Box<dyn ProviderOptionsData> =
        VolcengineOptions::new().with_thinking_budget(4096).boxed();

    let volcengine_opts = downcast_options::<VolcengineOptions>(&opts);
    assert!(volcengine_opts.is_some());
    assert_eq!(volcengine_opts.unwrap().thinking_budget_tokens, Some(4096));
}
