use super::*;

#[test]
fn test_compact_config_default() {
    let config = CompactConfig::default();
    // Feature toggles
    assert!(!config.disable_compact);
    assert!(!config.disable_auto_compact);
    assert!(!config.disable_micro_compact);
    // Overrides — default is 80% (matching Claude Code)
    assert_eq!(config.auto_compact_pct, Some(80));
    assert!(config.blocking_limit_override.is_none());
    // Session memory
    assert_eq!(
        config.session_memory_min_tokens,
        DEFAULT_SESSION_MEMORY_MIN_TOKENS
    );
    assert_eq!(
        config.session_memory_max_tokens,
        DEFAULT_SESSION_MEMORY_MAX_TOKENS
    );
    assert_eq!(
        config.extraction_cooldown_secs,
        DEFAULT_EXTRACTION_COOLDOWN_SECS
    );
    // Context restoration
    assert_eq!(
        config.context_restore_max_files,
        DEFAULT_CONTEXT_RESTORE_MAX_FILES
    );
    assert_eq!(
        config.context_restore_budget,
        DEFAULT_CONTEXT_RESTORE_BUDGET
    );
    assert_eq!(config.max_tokens_per_file, DEFAULT_MAX_TOKENS_PER_FILE);
    // Threshold control
    assert_eq!(
        config.min_tokens_to_preserve,
        DEFAULT_MIN_TOKENS_TO_PRESERVE
    );
    assert_eq!(
        config.warning_threshold_offset,
        DEFAULT_WARNING_THRESHOLD_OFFSET
    );
    assert_eq!(
        config.error_threshold_offset,
        DEFAULT_ERROR_THRESHOLD_OFFSET
    );
    assert_eq!(config.min_blocking_offset, DEFAULT_MIN_BLOCKING_OFFSET);
    // Micro-compact
    assert_eq!(
        config.micro_compact_min_savings,
        DEFAULT_MICRO_COMPACT_MIN_SAVINGS
    );
    assert_eq!(
        config.micro_compact_threshold,
        DEFAULT_MICRO_COMPACT_THRESHOLD
    );
    assert_eq!(
        config.recent_tool_results_to_keep,
        DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP
    );
    // Full compact
    assert_eq!(config.max_summary_retries, DEFAULT_MAX_SUMMARY_RETRIES);
    assert_eq!(
        config.max_compact_output_tokens,
        DEFAULT_MAX_COMPACT_OUTPUT_TOKENS
    );
    assert!((config.token_safety_margin - DEFAULT_TOKEN_SAFETY_MARGIN).abs() < f64::EPSILON);
    assert_eq!(config.tokens_per_image, DEFAULT_TOKENS_PER_IMAGE);
}

#[test]
fn test_compact_config_serde() {
    let json = r#"{
        "disable_compact": true,
        "disable_auto_compact": true,
        "auto_compact_pct": 80,
        "session_memory_min_tokens": 15000,
        "session_memory_max_tokens": 50000,
        "min_tokens_to_preserve": 15000,
        "micro_compact_min_savings": 25000
    }"#;
    let config: CompactConfig = serde_json::from_str(json).unwrap();
    assert!(config.disable_compact);
    assert!(config.disable_auto_compact);
    assert_eq!(config.auto_compact_pct, Some(80));
    assert_eq!(config.session_memory_min_tokens, 15000);
    assert_eq!(config.session_memory_max_tokens, 50000);
    assert_eq!(config.min_tokens_to_preserve, 15000);
    assert_eq!(config.micro_compact_min_savings, 25000);
}

#[test]
fn test_is_compaction_enabled() {
    let mut config = CompactConfig::default();
    assert!(config.is_compaction_enabled());

    config.disable_compact = true;
    assert!(!config.is_compaction_enabled());
}

#[test]
fn test_is_auto_compact_enabled() {
    let mut config = CompactConfig::default();
    assert!(config.is_auto_compact_enabled());

    config.disable_auto_compact = true;
    assert!(!config.is_auto_compact_enabled());

    config.disable_auto_compact = false;
    config.disable_compact = true;
    assert!(!config.is_auto_compact_enabled());
}

#[test]
fn test_is_micro_compact_enabled() {
    let mut config = CompactConfig::default();
    assert!(config.is_micro_compact_enabled());

    config.disable_micro_compact = true;
    assert!(!config.is_micro_compact_enabled());

    config.disable_micro_compact = false;
    config.disable_compact = true;
    assert!(!config.is_micro_compact_enabled());
}

#[test]
fn test_auto_compact_target() {
    let available = 200000;

    // Default has 80%, so target = 80% of 200000 = 160000
    let config = CompactConfig::default();
    let target = config.auto_compact_target(available);
    assert_eq!(target, 160000);

    // Without percentage override, target = available - min_tokens_to_preserve
    let mut config_no_pct = CompactConfig::default();
    config_no_pct.auto_compact_pct = None;
    let target = config_no_pct.auto_compact_target(available);
    assert_eq!(target, available - DEFAULT_MIN_TOKENS_TO_PRESERVE);

    // High percentage should be capped at available - min_tokens_to_preserve
    let mut config_high_pct = CompactConfig::default();
    config_high_pct.auto_compact_pct = Some(99);
    let target = config_high_pct.auto_compact_target(available);
    // 99% = 198000, but capped at 200000 - 13000 = 187000
    assert_eq!(target, 187000);
}

#[test]
fn test_blocking_limit() {
    let config = CompactConfig::default();
    let available = 200000;

    // Without override
    let limit = config.blocking_limit(available);
    assert_eq!(limit, available - DEFAULT_MIN_BLOCKING_OFFSET);

    // With override
    let mut config_with_override = CompactConfig::default();
    config_with_override.blocking_limit_override = Some(180000);
    let limit = config_with_override.blocking_limit(available);
    assert_eq!(limit, 180000);
}

#[test]
fn test_warning_and_error_thresholds() {
    let config = CompactConfig::default();
    let target = 180000;

    let warning = config.warning_threshold(target);
    assert_eq!(warning, target - DEFAULT_WARNING_THRESHOLD_OFFSET);

    let error = config.error_threshold(target);
    assert_eq!(error, target - DEFAULT_ERROR_THRESHOLD_OFFSET);
}

#[test]
fn test_estimate_tokens_with_margin() {
    let config = CompactConfig::default();

    let base = 10000;
    let with_margin = config.estimate_tokens_with_margin(base);
    // 10000 * 1.333... = 13333.33..., ceil = 13334
    assert_eq!(with_margin, 13334);
}

#[test]
fn test_validate_valid_config() {
    let config = CompactConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_pct() {
    let config = CompactConfig {
        auto_compact_pct: Some(150),
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_validate_min_greater_than_max() {
    let config = CompactConfig {
        session_memory_min_tokens: 50000,
        session_memory_max_tokens: 10000,
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_validate_negative_values() {
    // Test various negative value validations
    let test_cases = [
        CompactConfig {
            min_tokens_to_preserve: -1,
            ..Default::default()
        },
        CompactConfig {
            micro_compact_min_savings: -1,
            ..Default::default()
        },
        CompactConfig {
            recent_tool_results_to_keep: -1,
            ..Default::default()
        },
        CompactConfig {
            max_tokens_per_file: -1,
            ..Default::default()
        },
    ];

    for config in test_cases {
        assert!(config.validate().is_err());
    }
}

#[test]
fn test_validate_max_summary_retries() {
    let config = CompactConfig {
        max_summary_retries: 0,
        ..Default::default()
    };
    assert!(config.validate().is_err());

    let config = CompactConfig {
        max_summary_retries: 1,
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_token_safety_margin() {
    let config = CompactConfig {
        token_safety_margin: 0.9,
        ..Default::default()
    };
    assert!(config.validate().is_err());

    let config = CompactConfig {
        token_safety_margin: 1.0,
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}
