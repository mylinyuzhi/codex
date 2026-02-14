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
        keep_coding_instructions: None,
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
        keep_coding_instructions: None,
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
        keep_coding_instructions: None,
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
        keep_coding_instructions: None,
    };
    assert!(config.resolve_instruction().is_none());
}

#[test]
fn test_output_style_config_neither_set() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: None,
        instruction: None,
        keep_coding_instructions: None,
    };
    assert!(config.resolve_instruction().is_none());
}

#[test]
fn test_resolve_prompt_config_disabled() {
    let config = OutputStyleConfig {
        enabled: false,
        style_name: Some("explanatory".to_string()),
        instruction: None,
        keep_coding_instructions: None,
    };
    let tmp = std::env::temp_dir();
    assert!(config.resolve_prompt_config(&tmp).is_none());
}

#[test]
fn test_resolve_prompt_config_with_builtin_style() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("explanatory".to_string()),
        instruction: None,
        keep_coding_instructions: None,
    };
    let tmp = std::env::temp_dir();
    let result = config.resolve_prompt_config(&tmp).unwrap();
    assert_eq!(result.name, "explanatory");
    assert!(result.content.contains("Explanatory Style Active"));
    // Built-in styles default keep_coding_instructions to true
    assert!(result.keep_coding_instructions);
}

#[test]
fn test_resolve_prompt_config_with_custom_instruction() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: None,
        instruction: Some("My custom output style".to_string()),
        keep_coding_instructions: Some(true),
    };
    let tmp = std::env::temp_dir();
    let result = config.resolve_prompt_config(&tmp).unwrap();
    assert_eq!(result.name, "custom");
    assert_eq!(result.content, "My custom output style");
    assert!(result.keep_coding_instructions);
}

#[test]
fn test_resolve_prompt_config_keep_coding_override() {
    let config = OutputStyleConfig {
        enabled: true,
        style_name: Some("explanatory".to_string()),
        instruction: None,
        keep_coding_instructions: Some(false), // Override built-in default
    };
    let tmp = std::env::temp_dir();
    let result = config.resolve_prompt_config(&tmp).unwrap();
    // The override should take effect over the built-in default
    assert!(!result.keep_coding_instructions);
}
