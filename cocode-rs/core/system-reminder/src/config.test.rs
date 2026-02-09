use super::*;

#[test]
fn test_default_config() {
    let config = SystemReminderConfig::default();
    assert!(config.enabled);
    assert_eq!(config.timeout_ms, 1000);
    assert!(config.attachments.changed_files);
    assert!(config.attachments.plan_mode_enter);
    assert!(config.nested_memory.enabled);
}

#[test]
fn test_diagnostic_severity_ordering() {
    assert!(DiagnosticSeverity::Error < DiagnosticSeverity::Warning);
    assert!(DiagnosticSeverity::Warning < DiagnosticSeverity::Info);
    assert!(DiagnosticSeverity::Info < DiagnosticSeverity::Hint);
}

#[test]
fn test_serde_roundtrip() {
    let config = SystemReminderConfig {
        enabled: true,
        timeout_ms: 2000,
        critical_instruction: Some("Always be helpful".to_string()),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: SystemReminderConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.enabled, config.enabled);
    assert_eq!(parsed.timeout_ms, config.timeout_ms);
    assert_eq!(parsed.critical_instruction, config.critical_instruction);
}

#[test]
fn test_nested_memory_defaults() {
    let config = NestedMemoryConfig::default();
    assert!(config.enabled);
    assert_eq!(config.max_content_bytes, 40 * 1024);
    assert_eq!(config.max_lines, 3000);
    assert_eq!(config.max_import_depth, 5);
    assert!(config.patterns.contains(&"CLAUDE.md".to_string()));
}

#[test]
fn test_at_mentioned_files_defaults() {
    let config = AtMentionedFilesConfig::default();
    assert_eq!(config.max_file_size, 100 * 1024); // 100KB
    assert_eq!(config.max_lines, 2000);
    assert_eq!(config.max_line_length, 2000);
}

#[test]
fn test_output_style_config_defaults() {
    let config = OutputStyleConfig::default();
    assert!(!config.enabled);
    assert!(config.style_name.is_none());
    assert!(config.instruction.is_none());
}

#[test]
fn test_output_style_config_resolve_builtin() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("explanatory".to_string()),
        instruction: None,
    };
    let instruction = config.resolve_instruction().unwrap();
    assert!(instruction.contains("Explanatory Style Active"));
}

#[test]
fn test_output_style_config_custom_takes_precedence() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("explanatory".to_string()),
        instruction: Some("My custom style".to_string()),
    };
    let instruction = config.resolve_instruction().unwrap();
    assert_eq!(instruction, "My custom style");
}

#[test]
fn test_output_style_config_empty_instruction_fallback() {
    // Empty string instruction should fall back to style_name
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("learning".to_string()),
        instruction: Some(String::new()),
    };
    let instruction = config.resolve_instruction().unwrap();
    assert!(instruction.contains("Learning Style Active"));
}

#[test]
fn test_output_style_config_unknown_style() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("nonexistent".to_string()),
        instruction: None,
    };
    assert!(config.resolve_instruction().is_none());
}

#[test]
fn test_output_style_config_neither_set() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: None,
        instruction: None,
    };
    assert!(config.resolve_instruction().is_none());
}