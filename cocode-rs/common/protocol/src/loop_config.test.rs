use super::*;

#[test]
fn test_loop_config_default() {
    let config = LoopConfig::default();
    assert_eq!(config.max_turns, None);
    assert_eq!(config.max_tokens, None);
    assert_eq!(config.permission_mode, PermissionMode::Default);
    assert!(!config.enable_streaming_tools);
    assert!(!config.enable_micro_compaction);
    assert!(config.session_memory.enabled);
    assert!(config.stall_detection.enabled);
}

#[test]
fn test_session_memory_config_default() {
    let config = SessionMemoryConfig::default();
    assert_eq!(config.budget_tokens, 4096);
    assert_eq!(
        config.restoration_priority,
        FileRestorationPriority::MostRecent
    );
    assert!(config.enabled);
}

#[test]
fn test_stall_detection_config_default() {
    let config = StallDetectionConfig::default();
    assert_eq!(config.stall_timeout, Duration::from_secs(30));
    assert_eq!(config.recovery, StallRecovery::Retry);
    assert!(config.enabled);
}

#[test]
fn test_file_restoration_priority() {
    assert_eq!(FileRestorationPriority::MostRecent.as_str(), "most-recent");
    assert_eq!(
        FileRestorationPriority::MostAccessed.as_str(),
        "most-accessed"
    );
}

#[test]
fn test_stall_recovery() {
    assert_eq!(StallRecovery::Retry.as_str(), "retry");
    assert_eq!(StallRecovery::Abort.as_str(), "abort");
    assert_eq!(StallRecovery::Fallback.as_str(), "fallback");
}

#[test]
fn test_serde_roundtrip() {
    let config = LoopConfig {
        max_turns: Some(10),
        max_tokens: Some(100000),
        permission_mode: PermissionMode::AcceptEdits,
        enable_streaming_tools: true,
        enable_micro_compaction: true,
        fallback_model: Some("gpt-4".to_string()),
        agent_id: Some("agent-1".to_string()),
        parent_agent_id: None,
        record_sidechain: true,
        session_memory: SessionMemoryConfig::default(),
        stall_detection: StallDetectionConfig::default(),
        prompt_caching: PromptCachingConfig::default(),
    };

    let json = serde_json::to_string(&config).unwrap();
    let parsed: LoopConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.max_turns, config.max_turns);
    assert_eq!(parsed.max_tokens, config.max_tokens);
    assert_eq!(parsed.permission_mode, config.permission_mode);
    assert_eq!(parsed.enable_streaming_tools, config.enable_streaming_tools);
    assert_eq!(parsed.fallback_model, config.fallback_model);
}
