use super::*;
use crate::options::downcast_options;

#[test]
fn test_anthropic_options() {
    let opts = AnthropicOptions::new()
        .with_thinking_budget(10000)
        .with_user_id("user_123");

    assert_eq!(opts.thinking_budget_tokens, Some(10000));
    assert!(opts.metadata.is_some());
}

#[test]
fn test_downcast() {
    let opts: Box<dyn ProviderOptionsData> =
        AnthropicOptions::new().with_thinking_budget(5000).boxed();

    let anthropic_opts = downcast_options::<AnthropicOptions>(&opts);
    assert!(anthropic_opts.is_some());
    assert_eq!(anthropic_opts.unwrap().thinking_budget_tokens, Some(5000));
}
