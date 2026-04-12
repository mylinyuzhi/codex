use super::*;

#[test]
fn test_truncate_short_content() {
    let content = "# Memory\n- entry 1\n- entry 2";
    let result = truncate_entrypoint_content(content);
    assert_eq!(result, content);
}

#[test]
fn test_truncate_long_content() {
    let lines: Vec<String> = (0..250).map(|i| format!("- entry {i}")).collect();
    let content = lines.join("\n");
    let result = truncate_entrypoint_content(&content);
    assert!(result.lines().count() < 210); // 200 + truncation notice
    assert!(result.contains("truncated"));
}

#[test]
fn test_build_extract_auto_only_prompt() {
    let prompt = build_extract_auto_only_prompt(42, "## Existing\n- foo", false);
    assert!(prompt.contains("42 messages"));
    assert!(prompt.contains("## Existing"));
    assert!(prompt.contains("turn 1"));
    assert!(prompt.contains("turn 2"));
    assert!(prompt.contains("Step 1"));
    assert!(prompt.contains("Step 2"));
    assert!(prompt.contains("MEMORY.md"));
}

#[test]
fn test_build_extract_auto_only_skip_index() {
    let prompt = build_extract_auto_only_prompt(10, "", true);
    assert!(!prompt.contains("Step 2"));
    assert!(prompt.contains("no MEMORY.md indexing"));
}

#[test]
fn test_build_extract_combined_prompt() {
    let prompt = build_extract_combined_prompt(10, "personal manifest", "team manifest", false);
    assert!(prompt.contains("Personal Memories"));
    assert!(prompt.contains("Team Memories"));
    assert!(prompt.contains("API keys"));
    assert!(prompt.contains("scope"));
}

#[test]
fn test_build_daily_log_prompt() {
    let dir = std::path::Path::new("/home/.claude/memory");
    let prompt = build_daily_log_prompt("2026-04-06", dir);
    assert!(prompt.contains("2026"));
    assert!(prompt.contains("04"));
    assert!(prompt.contains("2026-04-06.md"));
    assert!(prompt.contains("append-only"));
}

#[test]
fn test_individual_types_section_has_xml_tags() {
    let section = build_individual_types_section();
    assert!(section.contains("<types>"));
    assert!(section.contains("<type>"));
    assert!(section.contains("<name>user</name>"));
    assert!(section.contains("<name>feedback</name>"));
    assert!(section.contains("<name>project</name>"));
    assert!(section.contains("<name>reference</name>"));
    assert!(section.contains("<description>"));
    assert!(section.contains("<when_to_save>"));
    assert!(section.contains("<how_to_use>"));
    assert!(section.contains("<examples>"));
    assert!(section.contains("</types>"));
}

#[test]
fn test_combined_types_section_has_scope() {
    let section = build_combined_types_section();
    assert!(section.contains("<scope>"));
    assert!(section.contains("always private"));
    assert!(section.contains("usually team"));
    assert!(section.contains("saves private user memory"));
    assert!(section.contains("saves team reference memory"));
}

#[test]
fn test_what_not_to_save_has_exclusion_paragraph() {
    let section = build_what_not_to_save_section();
    assert!(section.contains("These exclusions apply even when the user explicitly asks"));
    assert!(section.contains("surprising"));
}

#[test]
fn test_trusting_recall_section() {
    let section = build_trusting_recall_section();
    assert!(section.contains("Before recommending from memory"));
    assert!(section.contains("check the file exists"));
    assert!(section.contains("grep for it"));
    assert!(section.contains("\"The memory says X exists\""));
}

#[test]
fn test_save_instructions_two_step() {
    let config = MemoryConfig::default();
    let instructions = build_save_instructions(&config);
    assert!(instructions.contains("Step 1"));
    assert!(instructions.contains("Step 2"));
    assert!(instructions.contains("MEMORY.md"));
    assert!(instructions.contains("used to decide relevance"));
}

#[test]
fn test_save_instructions_skip_index() {
    let config = MemoryConfig {
        skip_index: true,
        ..MemoryConfig::default()
    };
    let instructions = build_save_instructions(&config);
    assert!(!instructions.contains("Step 2"));
}

#[test]
fn test_when_to_access_has_drift_caveat() {
    let section = build_when_to_access_section();
    assert!(section.contains("stale over time"));
    assert!(section.contains("verify"));
}

#[test]
fn test_persistence_section_has_plan_and_tasks() {
    let section = build_persistence_section();
    assert!(section.contains("Plan"));
    assert!(section.contains("Tasks"));
    assert!(section.contains("future conversations"));
}

use crate::config::MemoryConfig;
