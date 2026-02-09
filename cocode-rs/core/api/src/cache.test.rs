use super::*;

#[test]
fn test_cache_config_defaults() {
    let config = PromptCacheConfig::default();
    assert!(config.enabled);
    assert_eq!(config.min_tokens_for_cache, 1024);
    assert!(config.cache_system_prompt);
    assert!(config.cache_tools);
}

#[test]
fn test_cache_config_disabled() {
    let config = PromptCacheConfig::disabled();
    assert!(!config.enabled);
}

#[test]
fn test_cache_stats_from_usage() {
    // Cache miss
    let stats = CacheStats::from_usage(None, Some(100));
    assert!(!stats.is_hit);
    assert_eq!(stats.cache_creation_tokens, 100);

    // Cache hit
    let stats = CacheStats::from_usage(Some(1000), None);
    assert!(stats.is_hit);
    assert_eq!(stats.cache_read_tokens, 1000);
    assert!(stats.savings_ratio > 0.0);
}

#[test]
fn test_token_estimation() {
    let text = "Hello, world!"; // 13 chars -> ~3 tokens
    assert_eq!(text.estimate_tokens(), 3);

    let long_text = "a".repeat(4000); // 4000 chars -> 1000 tokens
    assert_eq!(long_text.estimate_tokens(), 1000);
}

#[test]
fn test_should_cache() {
    let config = PromptCacheConfig::default().with_min_tokens(100);

    let short_text = "Hello"; // ~1 token
    assert!(!short_text.should_cache(&config));

    let long_text = "a".repeat(500); // ~125 tokens
    assert!(long_text.should_cache(&config));
}

#[test]
fn test_find_cache_breakpoints() {
    let config = PromptCacheConfig::default().with_min_tokens(100);

    let messages = vec![
        Message::system("a".repeat(500)), // Should be cached
        Message::user("Hello"),
        Message::assistant("Hi there"),
    ];

    let breakpoints = find_cache_breakpoints(&messages, &config);
    assert!(!breakpoints.is_empty());
    assert!(breakpoints.contains(&0)); // System message
}

#[test]
fn test_find_cache_breakpoints_disabled() {
    let config = PromptCacheConfig::disabled();
    let messages = vec![Message::system("a".repeat(5000))];

    let breakpoints = find_cache_breakpoints(&messages, &config);
    assert!(breakpoints.is_empty());
}
