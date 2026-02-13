use super::*;
use crate::options::downcast_options;

#[test]
fn test_zai_options() {
    let opts = ZaiOptions::new()
        .with_thinking_budget(4096)
        .with_do_sample(true)
        .with_request_id("req_123")
        .with_user_id("user_456");

    assert_eq!(opts.thinking_budget_tokens, Some(4096));
    assert_eq!(opts.do_sample, Some(true));
    assert_eq!(opts.request_id, Some("req_123".to_string()));
    assert_eq!(opts.user_id, Some("user_456".to_string()));
}

#[test]
fn test_downcast() {
    let opts: Box<dyn ProviderOptionsData> = ZaiOptions::new().with_thinking_budget(8192).boxed();

    let zai_opts = downcast_options::<ZaiOptions>(&opts);
    assert!(zai_opts.is_some());
    assert_eq!(zai_opts.unwrap().thinking_budget_tokens, Some(8192));
}
