use super::*;
use crate::model::ReasoningEffort;

#[test]
fn test_merge_from() {
    let mut base = ModelInfo {
        display_name: Some("Base Model".to_string()),
        context_window: Some(4096),
        max_output_tokens: Some(1024),
        capabilities: Some(vec![Capability::TextGeneration]),
        temperature: Some(0.7),
        ..Default::default()
    };

    let other = ModelInfo {
        context_window: Some(8192),
        default_thinking_level: Some(ThinkingLevel::high()),
        temperature: Some(0.9),
        timeout_secs: Some(300),
        ..Default::default()
    };

    base.merge_from(&other);

    assert_eq!(base.display_name, Some("Base Model".to_string())); // Not overridden
    assert_eq!(base.context_window, Some(8192)); // Overridden
    assert_eq!(base.max_output_tokens, Some(1024)); // Not overridden
    assert_eq!(base.default_thinking_level, Some(ThinkingLevel::high())); // New value
    assert_eq!(base.temperature, Some(0.9)); // Overridden
    assert_eq!(base.timeout_secs, Some(300)); // New value
}

#[test]
fn test_has_capability() {
    let config = ModelInfo {
        capabilities: Some(vec![Capability::TextGeneration, Capability::Vision]),
        ..Default::default()
    };

    assert!(config.has_capability(Capability::TextGeneration));
    assert!(config.has_capability(Capability::Vision));
    assert!(!config.has_capability(Capability::Audio));
}

#[test]
fn test_builder() {
    let config = ModelInfo::new()
        .with_display_name("Test Model")
        .with_context_window(128000)
        .with_temperature(0.5)
        .with_timeout_secs(120)
        .with_capabilities(vec![Capability::TextGeneration, Capability::Streaming])
        .with_thinking_level(ThinkingLevel::medium());

    assert_eq!(config.display_name, Some("Test Model".to_string()));
    assert_eq!(config.context_window, Some(128000));
    assert_eq!(config.temperature, Some(0.5));
    assert_eq!(config.timeout_secs, Some(120));
    assert!(config.has_capability(Capability::Streaming));
    assert_eq!(config.default_thinking_level, Some(ThinkingLevel::medium()));
}

#[test]
fn test_serde() {
    let config = ModelInfo {
        display_name: Some("Test".to_string()),
        context_window: Some(4096),
        capabilities: Some(vec![Capability::TextGeneration]),
        temperature: Some(0.7),
        timeout_secs: Some(300),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: ModelInfo = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(config, parsed);
}

#[test]
fn test_nearest_supported_level() {
    let config = ModelInfo {
        supported_thinking_levels: Some(vec![
            ThinkingLevel::low(),
            ThinkingLevel::medium(),
            ThinkingLevel::high(),
        ]),
        ..Default::default()
    };

    // Exact match
    let result = config
        .nearest_supported_level(&ThinkingLevel::medium())
        .unwrap();
    assert_eq!(result.effort, ReasoningEffort::Medium);

    // None -> Low (nearest)
    let result = config
        .nearest_supported_level(&ThinkingLevel::none())
        .unwrap();
    assert_eq!(result.effort, ReasoningEffort::Low);

    // XHigh -> High (nearest)
    let result = config
        .nearest_supported_level(&ThinkingLevel::xhigh())
        .unwrap();
    assert_eq!(result.effort, ReasoningEffort::High);
}

#[test]
fn test_resolve_thinking_level() {
    let config = ModelInfo {
        supported_thinking_levels: Some(vec![
            ThinkingLevel::low(),
            ThinkingLevel::medium(),
            ThinkingLevel::high(),
        ]),
        ..Default::default()
    };

    // Exact match
    let result = config.resolve_thinking_level(&ThinkingLevel::medium());
    assert_eq!(result.effort, ReasoningEffort::Medium);

    // XHigh -> High (nearest)
    let result = config.resolve_thinking_level(&ThinkingLevel::xhigh());
    assert_eq!(result.effort, ReasoningEffort::High);
}

#[test]
fn test_resolve_thinking_level_no_supported() {
    let config = ModelInfo::default();

    // When no supported levels, return requested as-is
    let requested = ThinkingLevel::high();
    let result = config.resolve_thinking_level(&requested);
    assert_eq!(result, requested);
}

#[test]
fn test_merge_reasoning_summary() {
    use super::ReasoningSummary;

    let mut base = ModelInfo {
        reasoning_summary: Some(ReasoningSummary::Auto),
        ..Default::default()
    };

    let other = ModelInfo {
        reasoning_summary: Some(ReasoningSummary::Concise),
        ..Default::default()
    };

    base.merge_from(&other);

    assert_eq!(base.reasoning_summary, Some(ReasoningSummary::Concise));
}

#[test]
fn test_request_options_field_serde() {
    let mut opts = HashMap::new();
    opts.insert(
        "response_format".to_string(),
        serde_json::json!({"type": "json_object"}),
    );
    opts.insert("seed".to_string(), serde_json::json!(42));

    let config = ModelInfo {
        slug: "test-model".to_string(),
        options: Some(opts),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: ModelInfo = serde_json::from_str(&json).expect("deserialize");

    assert!(parsed.options.is_some());
    let parsed_opts = parsed.options.unwrap();
    assert_eq!(parsed_opts.get("seed"), Some(&serde_json::json!(42)));
    assert_eq!(
        parsed_opts.get("response_format"),
        Some(&serde_json::json!({"type": "json_object"}))
    );
}

#[test]
fn test_merge_from_request_options_maps() {
    let mut base_opts = HashMap::new();
    base_opts.insert("key1".to_string(), serde_json::json!("value1"));
    base_opts.insert("key2".to_string(), serde_json::json!("base_value"));

    let mut other_opts = HashMap::new();
    other_opts.insert("key2".to_string(), serde_json::json!("other_value")); // Override
    other_opts.insert("key3".to_string(), serde_json::json!("value3")); // New key

    let mut base = ModelInfo {
        options: Some(base_opts),
        ..Default::default()
    };

    let other = ModelInfo {
        options: Some(other_opts),
        ..Default::default()
    };

    base.merge_from(&other);

    let merged = base.options.unwrap();
    assert_eq!(merged.get("key1"), Some(&serde_json::json!("value1"))); // Preserved
    assert_eq!(merged.get("key2"), Some(&serde_json::json!("other_value"))); // Overridden
    assert_eq!(merged.get("key3"), Some(&serde_json::json!("value3"))); // Added
}

#[test]
fn test_merge_from_request_options_none_to_some() {
    let mut base = ModelInfo::default();
    assert!(base.options.is_none());

    let mut other_opts = HashMap::new();
    other_opts.insert("new_key".to_string(), serde_json::json!("new_value"));

    let other = ModelInfo {
        options: Some(other_opts),
        ..Default::default()
    };

    base.merge_from(&other);

    assert!(base.options.is_some());
    let merged = base.options.unwrap();
    assert_eq!(merged.get("new_key"), Some(&serde_json::json!("new_value")));
}

#[test]
fn test_get_request_option_helper() {
    let mut opts = HashMap::new();
    opts.insert("key".to_string(), serde_json::json!("value"));

    let config = ModelInfo {
        options: Some(opts),
        ..Default::default()
    };

    assert_eq!(
        config.get_request_option("key"),
        Some(&serde_json::json!("value"))
    );
    assert_eq!(config.get_request_option("nonexistent"), None);

    // None request_options
    let empty_config = ModelInfo::default();
    assert_eq!(empty_config.get_request_option("key"), None);
}

#[test]
fn test_with_request_options_builder() {
    let mut opts = HashMap::new();
    opts.insert(
        "response_format".to_string(),
        serde_json::json!({"type": "json_object"}),
    );

    let config = ModelInfo::new()
        .with_display_name("Test")
        .with_request_options(opts.clone());

    assert_eq!(config.options, Some(opts));
}

#[test]
fn test_merge_max_tool_output_chars() {
    let mut base = ModelInfo {
        max_tool_output_chars: Some(50_000),
        ..Default::default()
    };

    let other = ModelInfo {
        max_tool_output_chars: Some(20_000),
        ..Default::default()
    };

    base.merge_from(&other);
    assert_eq!(base.max_tool_output_chars, Some(20_000));
}

#[test]
fn test_merge_max_tool_output_chars_none_preserves() {
    let mut base = ModelInfo {
        max_tool_output_chars: Some(50_000),
        ..Default::default()
    };

    let other = ModelInfo::default(); // max_tool_output_chars is None

    base.merge_from(&other);
    assert_eq!(base.max_tool_output_chars, Some(50_000)); // Preserved
}

#[test]
fn test_max_tool_output_chars_serde() {
    let config = ModelInfo {
        slug: "test-model".to_string(),
        max_tool_output_chars: Some(30_000),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    assert!(json.contains("max_tool_output_chars"));
    let parsed: ModelInfo = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.max_tool_output_chars, Some(30_000));
}

#[test]
fn test_merge_excluded_tools_replaces() {
    let mut base = ModelInfo {
        excluded_tools: Some(vec!["Edit".to_string()]),
        ..Default::default()
    };

    let other = ModelInfo {
        excluded_tools: Some(vec!["Write".to_string(), "Read".to_string()]),
        ..Default::default()
    };

    base.merge_from(&other);
    assert_eq!(
        base.excluded_tools,
        Some(vec!["Write".to_string(), "Read".to_string()])
    );
}

#[test]
fn test_merge_excluded_tools_none_preserves() {
    let mut base = ModelInfo {
        excluded_tools: Some(vec!["Edit".to_string()]),
        ..Default::default()
    };

    let other = ModelInfo::default();
    base.merge_from(&other);
    assert_eq!(base.excluded_tools, Some(vec!["Edit".to_string()]));
}

#[test]
fn test_excluded_tools_serde() {
    let config = ModelInfo {
        slug: "test-model".to_string(),
        excluded_tools: Some(vec!["Edit".to_string(), "Write".to_string()]),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    assert!(json.contains("excluded_tools"));
    let parsed: ModelInfo = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        parsed.excluded_tools,
        Some(vec!["Edit".to_string(), "Write".to_string()])
    );
}
