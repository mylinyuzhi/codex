use super::*;

// ── classify_command tests ──

#[test]
fn test_classify_search_command() {
    let result = classify_command("grep -r 'pattern' src/");
    assert!(result.is_search);
    assert!(!result.is_read);
    assert!(!result.is_list);
    assert!(result.is_collapsible());
}

#[test]
fn test_classify_read_command() {
    let result = classify_command("cat file.rs");
    assert!(!result.is_search);
    assert!(result.is_read);
    assert!(!result.is_list);
    assert!(result.is_collapsible());
}

#[test]
fn test_classify_list_command() {
    let result = classify_command("ls -la");
    assert!(!result.is_search);
    assert!(!result.is_read);
    assert!(result.is_list);
    assert!(result.is_collapsible());
}

#[test]
fn test_classify_pipeline_mixed_search_and_read() {
    let result = classify_command("grep pattern file | sort | uniq");
    assert!(result.is_search);
    assert!(result.is_read);
    assert!(!result.is_list);
    assert!(result.is_collapsible());
}

#[test]
fn test_classify_non_collapsible_command() {
    let result = classify_command("npm install");
    assert!(!result.is_collapsible());
}

#[test]
fn test_classify_neutral_plus_read() {
    // echo is neutral, ls is list — overall should be list
    let result = classify_command("ls dir && echo '---' && ls dir2");
    assert!(result.is_list);
    assert!(result.is_collapsible());
}

#[test]
fn test_classify_only_neutral() {
    // Only neutral commands — not collapsible
    let result = classify_command("echo hello");
    assert!(!result.is_collapsible());
}

#[test]
fn test_classify_pipeline_with_non_read() {
    // cat is read but python is not — pipeline breaks collapsibility
    let result = classify_command("cat file | python process.py");
    assert!(!result.is_collapsible());
}

#[test]
fn test_classify_empty_command() {
    let result = classify_command("");
    assert!(!result.is_collapsible());
}

// ── is_silent_command tests ──

#[test]
fn test_silent_mv() {
    assert!(is_silent_command("mv a b"));
}

#[test]
fn test_silent_compound() {
    assert!(is_silent_command("mkdir -p dir && touch dir/file"));
}

#[test]
fn test_non_silent_echo() {
    assert!(!is_silent_command("echo hello"));
}

#[test]
fn test_non_silent_with_output() {
    assert!(!is_silent_command("cat file"));
}

// ── interpret_command_result tests ──

#[test]
fn test_interpret_grep_no_matches() {
    let result = interpret_command_result("grep pattern", 1, "", "");
    assert!(!result.is_error);
    assert_eq!(result.message.as_deref(), Some("No matches found"));
}

#[test]
fn test_interpret_grep_error() {
    let result = interpret_command_result("grep pattern", 2, "", "");
    assert!(result.is_error);
}

#[test]
fn test_interpret_diff_differences() {
    let result = interpret_command_result("diff a b", 1, "", "");
    assert!(!result.is_error);
    assert_eq!(result.message.as_deref(), Some("Files differ"));
}

#[test]
fn test_interpret_default_failure() {
    let result = interpret_command_result("cargo build", 101, "", "");
    assert!(result.is_error);
    assert!(result.message.unwrap().contains("101"));
}

// ── truncate_output_intelligent tests ──

#[test]
fn test_truncate_short_output() {
    let (result, truncated) = truncate_output_intelligent("hello\nworld\n", 1000);
    assert!(!truncated);
    assert_eq!(result, "hello\nworld");
}

#[test]
fn test_truncate_long_output() {
    let long = "a\n".repeat(100_000);
    let (result, truncated) = truncate_output_intelligent(&long, 100);
    assert!(truncated);
    assert!(result.contains("output truncated"));
    assert!(result.len() < long.len());
}

// ── strip_empty_lines tests ──

#[test]
fn test_strip_empty_lines_basic() {
    assert_eq!(strip_empty_lines("\n\nhello\nworld\n\n"), "hello\nworld");
}

#[test]
fn test_strip_empty_lines_all_empty() {
    assert_eq!(strip_empty_lines("\n\n\n"), "");
}

// ── is_image_output tests ──

#[test]
fn test_image_output_detection() {
    assert!(is_image_output("data:image/png;base64,iVBOR..."));
    assert!(!is_image_output("just some text"));
    assert!(!is_image_output("data:text/plain;base64,abc"));
}

// ── parse_data_uri tests ──

#[test]
fn test_parse_data_uri() {
    let (media, data) = parse_data_uri("data:image/png;base64,abc123").unwrap();
    assert_eq!(media, "image/png");
    assert_eq!(data, "abc123");
}

#[test]
fn test_parse_data_uri_invalid() {
    assert!(parse_data_uri("not a data uri").is_none());
}

// ── auto-backgrounding tests ──

#[test]
fn test_auto_background_allowed() {
    assert!(is_auto_backgrounding_allowed("npm install"));
    assert!(is_auto_backgrounding_allowed("cargo build"));
    assert!(!is_auto_backgrounding_allowed("sleep 10"));
}

// ── detect_blocked_sleep_pattern tests ──

#[test]
fn test_blocked_sleep_standalone() {
    let result = detect_blocked_sleep_pattern("sleep 5");
    assert_eq!(result.as_deref(), Some("standalone sleep 5"));
}

#[test]
fn test_blocked_sleep_with_followup() {
    let result = detect_blocked_sleep_pattern("sleep 5 && echo done");
    // split_simple gives ["sleep 5", "echo done"]
    // first = "sleep 5", secs=5 >= 2, rest = "echo done"
    assert_eq!(result.as_deref(), Some("sleep 5 followed by: echo done"));
}

#[test]
fn test_allowed_short_sleep() {
    // Sub-2s sleeps are fine
    assert!(detect_blocked_sleep_pattern("sleep 1").is_none());
}

// ── command_has_any_cd tests ──

#[test]
fn test_cd_detection() {
    assert!(command_has_any_cd("cd /tmp && ls"));
    assert!(command_has_any_cd("cd src"));
    assert!(!command_has_any_cd("cat cd_file.txt"));
}

// ── extract_description tests ──

#[test]
fn test_extract_description_with_provided() {
    assert_eq!(
        extract_description("ls -la", Some("List all files")),
        "List all files"
    );
}

#[test]
fn test_extract_description_fallback() {
    assert_eq!(extract_description("ls", None), "ls");
}

#[test]
fn test_extract_description_long_command() {
    let long_cmd = "a".repeat(200);
    let desc = extract_description(&long_cmd, None);
    assert!(desc.len() < 200);
    assert!(desc.ends_with("..."));
}

// ── progress tracker tests ──

#[test]
fn test_progress_tracker_initial_state() {
    let tracker = BashProgressTracker::new();
    assert!(!tracker.should_emit_progress());
    assert!(!tracker.was_progress_emitted());
}

// ── get_command_type_for_logging tests ──

#[test]
fn test_command_type_known() {
    assert_eq!(get_command_type_for_logging("npm install"), "npm");
    assert_eq!(get_command_type_for_logging("cargo build"), "cargo");
}

#[test]
fn test_command_type_unknown() {
    assert_eq!(get_command_type_for_logging("my-custom-tool"), "other");
}
