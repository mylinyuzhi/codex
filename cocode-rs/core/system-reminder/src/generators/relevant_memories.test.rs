use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::generator::AttachmentGenerator;

/// Default min keyword length matching the protocol constant.
const MIN_KW_LEN: usize = 3;

fn make_entry(
    path: &str,
    description: Option<&str>,
    memory_type: Option<&str>,
) -> cocode_auto_memory::AutoMemoryEntry {
    let frontmatter = if description.is_some() || memory_type.is_some() {
        Some(cocode_auto_memory::MemoryFrontmatter {
            name: None,
            description: description.map(String::from),
            memory_type: memory_type.map(String::from),
        })
    } else {
        None
    };
    cocode_auto_memory::AutoMemoryEntry {
        path: PathBuf::from(path),
        content: String::new(),
        frontmatter,
        last_modified: None,
        line_count: 0,
        was_truncated: false,
    }
}

#[test]
fn test_relevance_score_description_match() {
    let entry = make_entry(
        "debug.md",
        Some("Common debugging patterns for authentication"),
        None,
    );
    let score = compute_relevance_score(&entry, "help with authentication debugging", MIN_KW_LEN);
    assert!(score > 0, "Expected positive score for matching keywords");
}

#[test]
fn test_relevance_score_no_match() {
    let entry = make_entry("debug.md", Some("Debugging notes"), None);
    let score = compute_relevance_score(&entry, "xyz", MIN_KW_LEN);
    assert_eq!(score, 0, "Expected zero score for short/non-matching word");
}

#[test]
fn test_relevance_score_filename_match() {
    let entry = make_entry("authentication.md", None, None);
    let score = compute_relevance_score(&entry, "fix authentication issue", MIN_KW_LEN);
    assert!(score > 0, "Expected positive score for filename match");
}

#[test]
fn test_relevance_score_type_boost() {
    let entry = make_entry("notes.md", Some("Some notes"), Some("feedback"));
    let score = compute_relevance_score(&entry, "check feedback from user", MIN_KW_LEN);
    assert!(score > 0, "Expected positive score for type match");
}

#[test]
fn test_relevance_score_empty_prompt() {
    let entry = make_entry("debug.md", Some("Debug notes"), None);
    let score = compute_relevance_score(&entry, "", MIN_KW_LEN);
    assert_eq!(score, 0, "Expected zero score for empty prompt");
}

#[test]
fn test_relevance_score_short_words_filtered() {
    let entry = make_entry("debug.md", Some("A debugging guide"), None);
    // "is" and "a" are <3 chars, should be filtered
    let score = compute_relevance_score(&entry, "is a", MIN_KW_LEN);
    assert_eq!(score, 0, "Expected zero score when all words are too short");
}

#[test]
fn test_relevance_score_no_frontmatter() {
    let entry = make_entry("random.md", None, None);
    let score = compute_relevance_score(&entry, "something unrelated", MIN_KW_LEN);
    assert_eq!(score, 0);
}

// === Word boundary tests ===

#[test]
fn test_contains_word_exact_match() {
    assert!(contains_word("authentication debugging", "authentication"));
    assert!(contains_word("authentication debugging", "debugging"));
}

#[test]
fn test_contains_word_rejects_substring() {
    // "go" should NOT match "going" or "google"
    assert!(!contains_word("going to google", "go"));
    assert!(!contains_word("cargo build", "car"));
}

#[test]
fn test_contains_word_handles_punctuation_boundaries() {
    assert!(contains_word("auth-token", "auth"));
    assert!(contains_word("auth-token", "token"));
    assert!(contains_word("user.feedback", "feedback"));
    assert!(contains_word("project_notes", "project"));
    assert!(contains_word("project_notes", "notes"));
}

#[test]
fn test_relevance_score_no_false_positive_substring() {
    let entry = make_entry(
        "cargo.md",
        Some("Going to the store for our cargo management"),
        None,
    );
    // "go" is too short (<3), but "car" should NOT match "cargo" with word boundaries
    let score = compute_relevance_score(&entry, "car shopping", MIN_KW_LEN);
    assert_eq!(score, 0, "Substring 'car' should not match 'cargo'");
}

#[test]
fn test_relevance_score_hyphenated_word_match() {
    let entry = make_entry(
        "auth-guide.md",
        Some("Authentication and authorization patterns"),
        None,
    );
    let score = compute_relevance_score(&entry, "fix auth issue", MIN_KW_LEN);
    // "auth" matches filename token "auth" (+1) and description token "authentication"? No —
    // "auth" != "authentication". Only exact token match counts.
    assert_eq!(score, 1, "Should match 'auth' in filename only");
}

// === Index deduplication tests ===

#[tokio::test]
async fn test_extract_index_filenames_from_markdown_links() {
    let config = cocode_auto_memory::config::ResolvedAutoMemoryConfig {
        enabled: true,
        directory: PathBuf::from("/tmp/test-memory"),
        max_lines: 200,
        max_relevant_files: 5,
        max_lines_per_file: 200,
        relevant_search_timeout_ms: 5000,
        relevant_memories_enabled: true,
        memory_extraction_enabled: false,
        max_frontmatter_lines: 20,
        staleness_warning_days: 1,
        relevant_memories_throttle_turns: 3,
        max_files_to_scan: 200,
        min_keyword_length: 3,
        disable_reason: None,
    };

    let state = cocode_auto_memory::AutoMemoryState::new(config);

    // Simulate a MEMORY.md index with linked files
    // (We can't easily set the index directly, so test the extraction logic)
    let filenames = extract_index_filenames(&state).await;
    // With no index loaded, should return empty set
    assert!(filenames.is_empty());
}

#[test]
fn test_extract_index_filenames_parsing() {
    // Test the parsing logic directly via a mock index
    let index_content = "# Memory Index\n\n- [user role](user_role.md) - User preferences\n- [feedback](feedback_testing.md) - Testing feedback\n- bare_ref.md\n";

    let filenames: std::collections::HashSet<String> = index_content
        .split(|c: char| c == '(' || c == ')' || c == '[' || c == ']' || c.is_whitespace())
        .filter(|s| s.ends_with(".md") && !s.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    assert!(filenames.contains("user_role.md"));
    assert!(filenames.contains("feedback_testing.md"));
    assert!(filenames.contains("bare_ref.md"));
    assert!(!filenames.contains("MEMORY.md")); // Not in the content
}

// === Throttle config tests ===

#[test]
fn test_throttle_config_uses_default_without_context() {
    let generator = RelevantMemoriesGenerator;
    let config = generator.throttle_config();
    assert_eq!(
        config.min_turns_between,
        cocode_protocol::DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS
    );
}

#[test]
fn test_throttle_config_reads_from_auto_memory_state() {
    let generator = RelevantMemoriesGenerator;

    // Create config with custom throttle value
    let mut auto_config = cocode_auto_memory::config::ResolvedAutoMemoryConfig {
        enabled: true,
        directory: PathBuf::from("/tmp/test-memory"),
        max_lines: 200,
        max_relevant_files: 5,
        max_lines_per_file: 200,
        relevant_search_timeout_ms: 5000,
        relevant_memories_enabled: true,
        memory_extraction_enabled: false,
        max_frontmatter_lines: 20,
        staleness_warning_days: 1,
        relevant_memories_throttle_turns: 7, // custom value
        max_files_to_scan: 200,
        min_keyword_length: 3,
        disable_reason: None,
    };

    let state = Arc::new(cocode_auto_memory::AutoMemoryState::new(
        auto_config.clone(),
    ));
    let sr_config = crate::config::SystemReminderConfig::default();
    let ctx = crate::generator::GeneratorContext::builder()
        .config(&sr_config)
        .cwd(PathBuf::from("/tmp"))
        .auto_memory_state(state)
        .build();

    let config = generator.throttle_config_for_context(&ctx);
    assert_eq!(
        config.min_turns_between, 7,
        "Should use custom throttle value from config"
    );

    // Verify different value propagates
    auto_config.relevant_memories_throttle_turns = 10;
    let state2 = Arc::new(cocode_auto_memory::AutoMemoryState::new(auto_config));
    let ctx2 = crate::generator::GeneratorContext::builder()
        .config(&sr_config)
        .cwd(PathBuf::from("/tmp"))
        .auto_memory_state(state2)
        .build();
    let config2 = generator.throttle_config_for_context(&ctx2);
    assert_eq!(config2.min_turns_between, 10);
}

#[test]
fn test_throttle_config_falls_back_without_state() {
    let generator = RelevantMemoriesGenerator;
    let sr_config = crate::config::SystemReminderConfig::default();
    let ctx = crate::generator::GeneratorContext::builder()
        .config(&sr_config)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let config = generator.throttle_config_for_context(&ctx);
    assert_eq!(
        config.min_turns_between,
        cocode_protocol::DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS,
        "Should fall back to default when no auto_memory_state"
    );
}
