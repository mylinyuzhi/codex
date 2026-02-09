use super::*;

#[test]
fn test_usage_default() {
    let usage = Usage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
    assert_eq!(usage.reasoning_tokens(), 0);
    assert_eq!(usage.cached_tokens(), 0);
}

#[test]
fn test_usage_with_details() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        input_tokens_details: InputTokensDetails {
            cached_tokens: 20,
            text_tokens: 60,
            image_tokens: 20,
            audio_tokens: 0,
        },
        output_tokens_details: OutputTokensDetails {
            reasoning_tokens: 30,
            text_tokens: 20,
            audio_tokens: 0,
        },
    };
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.reasoning_tokens(), 30);
    assert_eq!(usage.cached_tokens(), 20);
    assert_eq!(usage.input_text_tokens(), 60);
    assert_eq!(usage.image_tokens(), 20);
}

#[test]
fn test_usage_serde() {
    let json = r#"{
        "input_tokens": 100,
        "output_tokens": 50,
        "total_tokens": 150,
        "input_tokens_details": {"cached_tokens": 20, "text_tokens": 60},
        "output_tokens_details": {"reasoning_tokens": 30}
    }"#;
    let usage: Usage = serde_json::from_str(json).unwrap();
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.reasoning_tokens(), 30);
    assert_eq!(usage.cached_tokens(), 20);
}

#[test]
fn test_usage_serde_missing_details() {
    // Test that missing details default to 0
    let json = r#"{"input_tokens": 100, "output_tokens": 50, "total_tokens": 150}"#;
    let usage: Usage = serde_json::from_str(json).unwrap();
    assert_eq!(usage.reasoning_tokens(), 0);
    assert_eq!(usage.cached_tokens(), 0);
}
