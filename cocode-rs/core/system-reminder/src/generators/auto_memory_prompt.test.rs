use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::GeneratorContext;

fn test_auto_memory_config(
    enabled: bool,
    memory_extraction_enabled: bool,
) -> cocode_auto_memory::ResolvedAutoMemoryConfig {
    cocode_auto_memory::ResolvedAutoMemoryConfig {
        enabled,
        directory: std::path::PathBuf::from("/tmp/test-memory"),
        max_lines: 200,
        max_relevant_files: 5,
        max_lines_per_file: 200,
        relevant_search_timeout_ms: 5000,
        relevant_memories_enabled: false,
        memory_extraction_enabled,
        max_frontmatter_lines: 20,
        staleness_warning_days: 1,
        relevant_memories_throttle_turns: 3,
        max_files_to_scan: 200,
        min_keyword_length: 3,
        disable_reason: None,
    }
}

#[tokio::test]
async fn test_returns_none_when_no_state() {
    let config = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .build();

    let generator = AutoMemoryPromptGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_when_disabled() {
    let config = SystemReminderConfig::default();
    let state = cocode_auto_memory::AutoMemoryState::new_arc(test_auto_memory_config(false, false));
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .auto_memory_state(state)
        .build();

    let generator = AutoMemoryPromptGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_main_agent_gets_full_prompt() {
    let config = SystemReminderConfig::default();
    let state = cocode_auto_memory::AutoMemoryState::new_arc(test_auto_memory_config(true, false));
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_main_agent(true)
        .auto_memory_state(state)
        .build();

    let generator = AutoMemoryPromptGenerator;
    let result = generator.generate(&ctx).await.unwrap().unwrap();
    let content = result.output.as_text().unwrap();
    // Full prompt contains save instructions
    assert!(content.contains("How to save memories"));
    assert!(!content.contains("should not write to memory files yourself"));
}

#[tokio::test]
async fn test_subagent_gets_readonly_prompt() {
    let config = SystemReminderConfig::default();
    let state = cocode_auto_memory::AutoMemoryState::new_arc(test_auto_memory_config(true, false));
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_main_agent(false)
        .auto_memory_state(state)
        .build();

    let generator = AutoMemoryPromptGenerator;
    let result = generator.generate(&ctx).await.unwrap().unwrap();
    let content = result.output.as_text().unwrap();
    // Read-only prompt for subagents
    assert!(content.contains("should not write to memory files yourself"));
    assert!(!content.contains("How to save memories"));
}

#[tokio::test]
async fn test_extraction_mode_gets_readonly_prompt() {
    let config = SystemReminderConfig::default();
    let state = cocode_auto_memory::AutoMemoryState::new_arc(test_auto_memory_config(true, true));
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_main_agent(true)
        .auto_memory_state(state)
        .build();

    let generator = AutoMemoryPromptGenerator;
    let result = generator.generate(&ctx).await.unwrap().unwrap();
    let content = result.output.as_text().unwrap();
    // When extraction is enabled, main agent also gets read-only prompt
    assert!(content.contains("should not write to memory files yourself"));
}
