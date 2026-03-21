use super::*;

#[test]
fn test_build_auto_memory_prompt_no_index() {
    let prompt = build_auto_memory_prompt("/home/.cocode/projects/abc/memory", None, 200);
    assert!(prompt.contains("# auto memory"));
    assert!(prompt.contains("/home/.cocode/projects/abc/memory"));
    assert!(prompt.contains("Types of memory"));
    assert!(prompt.contains("How to save memories"));
    assert!(prompt.contains("Searching memory files"));
    // Verify max_lines is used dynamically
    assert!(prompt.contains("after 200"));
}

#[test]
fn test_build_auto_memory_prompt_custom_max_lines() {
    let prompt = build_auto_memory_prompt("/memory", None, 100);
    assert!(prompt.contains("after 100"));
    assert!(!prompt.contains("after 200"));
}

#[test]
fn test_build_auto_memory_prompt_with_index() {
    let index = MemoryIndex {
        raw_content: "- [debug](debug.md) - Debugging notes".to_string(),
        line_count: 1,
        was_truncated: false,
        last_modified: None,
    };
    let prompt = build_auto_memory_prompt("/memory", Some(&index), 200);
    assert!(prompt.contains("debug.md"));
    assert!(prompt.contains("Debugging notes"));
}

#[test]
fn test_build_auto_memory_prompt_truncated_index() {
    let index = MemoryIndex {
        raw_content: "some content".to_string(),
        line_count: 250,
        was_truncated: true,
        last_modified: None,
    };
    let prompt = build_auto_memory_prompt("/memory", Some(&index), 150);
    assert!(prompt.contains("250 lines"));
    assert!(prompt.contains("first 150 are shown"));
}

#[test]
fn test_build_background_agent_prompt() {
    let prompt = build_background_agent_memory_prompt("/memory", 200);
    assert!(prompt.contains("background agent automatically extracts"));
    assert!(prompt.contains("should not write to memory files yourself"));
    assert!(prompt.contains("When to access memories"));
    assert!(prompt.contains("Searching memory files"));
    assert!(prompt.contains("first 200 lines"));
}

#[test]
fn test_build_background_agent_prompt_custom_max_lines() {
    let prompt = build_background_agent_memory_prompt("/memory", 300);
    assert!(prompt.contains("first 300 lines"));
}

#[test]
fn test_search_context_section_included() {
    let prompt = build_auto_memory_prompt("/my/memory", None, 200);
    assert!(prompt.contains("Searching memory files"));
    assert!(prompt.contains("/my/memory"));
    assert!(prompt.contains("Grep tool"));
    assert!(prompt.contains("Read tool"));
}

// --- New tests for XML-structured memory types ---

#[test]
fn test_prompt_contains_xml_memory_types() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    // Verify XML structure
    assert!(prompt.contains("<types>"));
    assert!(prompt.contains("</types>"));
    // Verify all 4 types
    assert!(prompt.contains("<name>user</name>"));
    assert!(prompt.contains("<name>feedback</name>"));
    assert!(prompt.contains("<name>project</name>"));
    assert!(prompt.contains("<name>reference</name>"));
    // Verify sub-elements
    assert!(prompt.contains("<description>"));
    assert!(prompt.contains("<when_to_save>"));
    assert!(prompt.contains("<how_to_use>"));
    assert!(prompt.contains("<examples>"));
}

#[test]
fn test_prompt_contains_body_structure() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    // feedback and project types have body_structure
    assert!(prompt.contains("<body_structure>"));
    assert!(prompt.contains("Lead with the rule itself"));
    assert!(prompt.contains("**Why:**"));
    assert!(prompt.contains("**How to apply:**"));
}

#[test]
fn test_prompt_contains_save_immediately_guidance() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("save it immediately"));
}

#[test]
fn test_prompt_contains_forget_guidance() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("find and remove the relevant entry"));
}

#[test]
fn test_prompt_contains_build_over_time() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("build up this memory system over time"));
}

#[test]
fn test_prompt_contains_date_conversion() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("convert relative dates"));
}

#[test]
fn test_prompt_contains_record_from_success() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("Record from failure AND success"));
}

#[test]
fn test_prompt_contains_truncation_guidance() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("lines after 200"));
    assert!(prompt.contains("will be truncated"));
}

#[test]
fn test_prompt_contains_staleness_snapshot_guidance() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("frozen in time"));
    assert!(prompt.contains("prefer `git log`"));
}

#[test]
fn test_prompt_what_not_to_save_section() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("What NOT to save in memory"));
    assert!(prompt.contains("Code patterns, conventions"));
    assert!(prompt.contains("ask what was *surprising*"));
}

#[test]
fn test_prompt_memory_vs_plan_vs_task() {
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(prompt.contains("Memory and other forms of persistence"));
    assert!(prompt.contains("persist that change by updating the plan"));
}

#[test]
fn test_prompt_empty_memory_index_shows_hint() {
    let index = MemoryIndex {
        raw_content: String::new(),
        line_count: 0,
        was_truncated: false,
        last_modified: None,
    };
    let prompt = build_auto_memory_prompt("/memory", Some(&index), 200);
    assert!(
        prompt.contains("currently empty"),
        "Empty MEMORY.md should show a hint"
    );
    assert!(prompt.contains("When you save new memories"));
}

#[test]
fn test_prompt_whitespace_only_memory_index_shows_hint() {
    let index = MemoryIndex {
        raw_content: "   \n  \n".to_string(),
        line_count: 2,
        was_truncated: false,
        last_modified: None,
    };
    let prompt = build_auto_memory_prompt("/memory", Some(&index), 200);
    assert!(
        prompt.contains("currently empty"),
        "Whitespace-only MEMORY.md should be treated as empty"
    );
}

#[test]
fn test_prompt_no_index_file_no_hint() {
    // When MEMORY.md doesn't exist (None), no empty hint should appear
    let prompt = build_auto_memory_prompt("/memory", None, 200);
    assert!(
        !prompt.contains("currently empty"),
        "Missing MEMORY.md should not show empty hint"
    );
}
