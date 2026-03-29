use super::*;

fn mock_llm(response: &str) -> LlmCallFn {
    let response = response.to_string();
    Arc::new(move |_system, _user| {
        let r = response.clone();
        Box::pin(async move { Ok(r) })
    })
}

fn mock_llm_error() -> LlmCallFn {
    Arc::new(|_system, _user| Box::pin(async { Err("model error".to_string()) }))
}

#[test]
fn test_parse_prefix_response_prefix() {
    assert_eq!(
        parse_prefix_response("git diff", "git diff HEAD~1"),
        PrefixResult::Prefix("git diff".to_string())
    );
}

#[test]
fn test_parse_prefix_response_none() {
    assert_eq!(
        parse_prefix_response("none", "npm run lint"),
        PrefixResult::NoneExtracted
    );
}

#[test]
fn test_parse_prefix_response_injection() {
    assert_eq!(
        parse_prefix_response("command_injection_detected", "git status`ls`"),
        PrefixResult::InjectionDetected
    );
}

#[test]
fn test_parse_prefix_response_invalid_prefix() {
    // Response is not a prefix of the command
    assert_eq!(
        parse_prefix_response("npm test", "git diff HEAD"),
        PrefixResult::NoneExtracted
    );
}

#[test]
fn test_parse_prefix_response_empty() {
    assert_eq!(
        parse_prefix_response("", "ls -la"),
        PrefixResult::NoneExtracted
    );
}

#[tokio::test]
async fn test_extract_prefix_cached() {
    let call_count = Arc::new(std::sync::atomic::AtomicI32::new(0));
    let count = call_count.clone();
    let llm: LlmCallFn = Arc::new(move |_system, _user| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok("git diff".to_string()) })
    });

    let extractor = PrefixExtractor::new(llm);

    // First call — cache miss
    let r1 = extractor.extract_prefix("git diff HEAD~1").await;
    assert_eq!(r1, PrefixResult::Prefix("git diff".to_string()));

    // Second call — cache hit (LLM not called again)
    let r2 = extractor.extract_prefix("git diff HEAD~1").await;
    assert_eq!(r2, PrefixResult::Prefix("git diff".to_string()));

    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_extract_prefix_error() {
    let extractor = PrefixExtractor::new(mock_llm_error());
    let result = extractor.extract_prefix("ls").await;
    assert!(matches!(result, PrefixResult::Error(_)));
}

#[tokio::test]
async fn test_clear_cache() {
    let extractor = PrefixExtractor::new(mock_llm("cat"));
    extractor.extract_prefix("cat foo.txt").await;
    extractor.clear_cache().await;
    // After clear, next call should be a cache miss (LLM called again)
    let result = extractor.extract_prefix("cat foo.txt").await;
    assert_eq!(result, PrefixResult::Prefix("cat".to_string()));
}

#[test]
fn test_prefix_result_methods() {
    assert_eq!(
        PrefixResult::Prefix("git diff".to_string()).prefix(),
        Some("git diff")
    );
    assert_eq!(PrefixResult::NoneExtracted.prefix(), None);
    assert!(PrefixResult::InjectionDetected.is_injection());
    assert!(!PrefixResult::NoneExtracted.is_injection());
}
