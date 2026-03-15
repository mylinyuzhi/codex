use super::*;

#[test]
fn converts_full_usage() {
    let google_usage = GoogleUsageMetadata {
        prompt_token_count: Some(100),
        candidates_token_count: Some(200),
        cached_content_token_count: Some(50),
        thoughts_token_count: Some(30),
        total_token_count: Some(300),
    };
    let usage = convert_usage(Some(&google_usage));
    assert_eq!(usage.total_input_tokens(), 100);
    assert_eq!(usage.total_output_tokens(), 200);
    assert_eq!(usage.input_tokens.cache_read, Some(50));
    assert_eq!(usage.output_tokens.reasoning, Some(30));
}

#[test]
fn handles_none_usage() {
    let usage = convert_usage(None);
    assert_eq!(usage.total_input_tokens(), 0);
    assert_eq!(usage.total_output_tokens(), 0);
}

#[test]
fn handles_partial_usage() {
    let google_usage = GoogleUsageMetadata {
        prompt_token_count: Some(100),
        candidates_token_count: None,
        cached_content_token_count: None,
        thoughts_token_count: None,
        total_token_count: None,
    };
    let usage = convert_usage(Some(&google_usage));
    assert_eq!(usage.total_input_tokens(), 100);
    assert_eq!(usage.total_output_tokens(), 0);
}
