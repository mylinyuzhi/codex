use super::*;

#[test]
fn test_default_config() {
    let config = AutoMemoryConfig::default();
    assert_eq!(config.enabled, None);
    assert_eq!(config.directory, None);
    assert_eq!(config.max_lines, 200);
    assert_eq!(config.max_relevant_files, 5);
    assert_eq!(config.max_lines_per_file, 200);
    assert_eq!(config.relevant_search_timeout_ms, 5000);
}

#[test]
fn test_json_roundtrip() {
    let config = AutoMemoryConfig {
        enabled: Some(true),
        directory: Some("/custom/path".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let parsed: AutoMemoryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, parsed);
}

#[test]
fn test_json_deserialize_minimal() {
    let json = r#"{}"#;
    let config: AutoMemoryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config, AutoMemoryConfig::default());
}
