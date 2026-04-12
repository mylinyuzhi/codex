use super::*;

#[test]
fn builds_empty_metadata() {
    assert!(build_responses_provider_metadata(None, None).is_none());
}

#[test]
fn builds_with_response_id() {
    let meta = build_responses_provider_metadata(Some("resp_123"), None).expect("should be Some");
    assert_eq!(meta.0["openai"]["responseId"], "resp_123");
}

#[test]
fn builds_with_both() {
    let meta =
        build_responses_provider_metadata(Some("resp_123"), Some("flex")).expect("should be Some");
    assert_eq!(meta.0["openai"]["responseId"], "resp_123");
    assert_eq!(meta.0["openai"]["serviceTier"], "flex");
}
