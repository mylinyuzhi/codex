use super::*;

#[test]
fn test_auto_search_config_default() {
    let config = McpAutoSearchConfig::default();
    assert!(config.enabled);
    assert!((config.context_threshold - 0.10).abs() < f32::EPSILON);
    assert_eq!(config.min_context_window, 32000);
    assert!(config.search_on_list_changed);
    assert!((config.chars_per_token - 2.5).abs() < f32::EPSILON);
}

#[test]
fn test_char_threshold() {
    let config = McpAutoSearchConfig::default();

    // 200k context: threshold = 0.1 * 200000 * 2.5 = 50000
    assert_eq!(config.char_threshold(200000), 50000);

    // 128k context: threshold = 0.1 * 128000 * 2.5 = 32000
    assert_eq!(config.char_threshold(128000), 32000);
}

#[test]
fn test_should_use_auto_search() {
    let config = McpAutoSearchConfig::default();

    // Disabled
    let disabled = McpAutoSearchConfig {
        enabled: false,
        ..Default::default()
    };
    assert!(!disabled.should_use_auto_search(200000, 100000, true));

    // No tool calling
    assert!(!config.should_use_auto_search(200000, 100000, false));

    // Context too small
    assert!(!config.should_use_auto_search(16000, 100000, true));

    // Below threshold (200k context, threshold = 50k chars)
    assert!(!config.should_use_auto_search(200000, 40000, true));

    // Above threshold
    assert!(config.should_use_auto_search(200000, 60000, true));

    // Exactly at threshold
    assert!(config.should_use_auto_search(200000, 50000, true));
}

#[test]
fn test_tool_cache_config_default() {
    let config = McpToolCacheConfig::default();
    assert!(config.enabled);
    assert_eq!(config.ttl_secs, 300);
    assert!(config.invalidate_on_list_changed);
}

#[test]
fn test_tool_cache_ttl() {
    let config = McpToolCacheConfig::default();
    assert_eq!(config.ttl(), std::time::Duration::from_secs(300));
}

#[test]
fn test_mcp_config_default() {
    let config = McpConfig::default();
    assert!(config.auto_search.enabled);
    assert!(config.tool_cache.enabled);
}

#[test]
fn test_validate_valid_config() {
    let config = McpConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_threshold() {
    let config = McpAutoSearchConfig {
        context_threshold: 1.5,
        ..Default::default()
    };
    assert!(config.validate().is_err());

    let config = McpAutoSearchConfig {
        context_threshold: -0.1,
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_validate_invalid_min_context() {
    let config = McpAutoSearchConfig {
        min_context_window: -1000,
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_validate_invalid_ttl() {
    let config = McpToolCacheConfig {
        ttl_secs: -10,
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_serde_roundtrip() {
    let config = McpConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let parsed: McpConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn test_serde_partial() {
    // Test that we can parse partial config with defaults
    let json = r#"{
        "auto_search": {
            "enabled": false
        }
    }"#;
    let config: McpConfig = serde_json::from_str(json).unwrap();
    assert!(!config.auto_search.enabled);
    assert!(config.tool_cache.enabled); // Default
}
