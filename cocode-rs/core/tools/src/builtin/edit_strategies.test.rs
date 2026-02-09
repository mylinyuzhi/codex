use super::*;

// ── Exact ───────────────────────────────────────────────────────

#[test]
fn test_exact_replace_basic() {
    let (result, count) = try_exact_replace("hello world", "world", "rust", false).unwrap();
    assert_eq!(result, "hello rust");
    assert_eq!(count, 1);
}

#[test]
fn test_exact_replace_all() {
    let (result, count) = try_exact_replace("foo bar foo", "foo", "baz", true).unwrap();
    assert_eq!(result, "baz bar baz");
    assert_eq!(count, 2);
}

#[test]
fn test_exact_no_match() {
    assert!(try_exact_replace("hello", "xyz", "abc", false).is_none());
}

// ── Flexible ────────────────────────────────────────────────────

#[test]
fn test_flexible_replace_basic() {
    let content = "    let x = 1;\n    let y = 2;\n";
    let old = "let x = 1;\nlet y = 2;";
    let new = "let x = 10;\nlet y = 20;";
    let (result, count) = try_flexible_replace(content, old, new, false).unwrap();
    assert_eq!(count, 1);
    assert!(result.contains("    let x = 10;"));
    assert!(result.contains("    let y = 20;"));
}

#[test]
fn test_flexible_replace_no_match() {
    assert!(try_flexible_replace("hello world\n", "nonexistent", "x", false).is_none());
}

#[test]
fn test_flexible_replace_all_occurrences() {
    let content = "    foo bar\n    baz\n    foo bar\n    baz\n";
    let (result, count) =
        try_flexible_replace(content, "foo bar\nbaz", "replaced\nline", true).unwrap();
    assert_eq!(count, 2);
    assert_eq!(result.matches("replaced").count(), 2);
}

// ── Regex ───────────────────────────────────────────────────────

#[test]
fn test_regex_replace_intra_line_whitespace() {
    let content = "function test(){body}";
    let old = "function test ( ) { body }";
    let new = "function test(){updated}";
    let (result, count) = try_regex_replace(content, old, new).unwrap();
    assert_eq!(count, 1);
    assert!(result.contains("updated"));
}

#[test]
fn test_regex_replace_first_only() {
    let content = "func(){}\nfunc(){}\n";
    let old = "func ( ) { }";
    let new = "updated(){}";
    let (result, count) = try_regex_replace(content, old, new).unwrap();
    assert_eq!(count, 1);
    // Only first occurrence replaced
    assert_eq!(result.matches("func(){}").count(), 1);
    assert!(result.contains("updated(){}"));
}

#[test]
fn test_regex_replace_no_match() {
    assert!(try_regex_replace("hello world", "nonexistent_func()", "x").is_none());
}

// ── Pre-correction ──────────────────────────────────────────────

#[test]
fn test_pre_correct_no_change() {
    let (old, new) = pre_correct_escaping("hello", "hi", "hello world");
    assert_eq!(old, "hello");
    assert_eq!(new, "hi");
}

#[test]
fn test_pre_correct_unescape_fixes_match() {
    let content = "line1\nline2";
    let (old, new) = pre_correct_escaping("line1\\nline2", "line1\\nupdated", content);
    assert_eq!(old, "line1\nline2");
    assert_eq!(new, "line1\nupdated");
}

#[test]
fn test_pre_correct_new_string_over_escaped() {
    let content = "hello world";
    let (old, new) = pre_correct_escaping("hello", "hi\\nthere", content);
    assert_eq!(old, "hello");
    assert_eq!(new, "hi\nthere");
}

#[test]
fn test_pre_correct_no_help() {
    let (old, new) = pre_correct_escaping("notfound", "replacement", "hello world");
    assert_eq!(old, "notfound");
    assert_eq!(new, "replacement");
}

// ── Unescape ────────────────────────────────────────────────────

#[test]
fn test_unescape_no_escapes() {
    assert_eq!(unescape_string_for_llm_bug("hello world"), "hello world");
}

#[test]
fn test_unescape_newline() {
    assert_eq!(unescape_string_for_llm_bug("line1\\nline2"), "line1\nline2");
}

#[test]
fn test_unescape_tab() {
    assert_eq!(unescape_string_for_llm_bug("col1\\tcol2"), "col1\tcol2");
}

#[test]
fn test_unescape_quotes() {
    assert_eq!(
        unescape_string_for_llm_bug("say \\\"hello\\\""),
        "say \"hello\""
    );
    assert_eq!(
        unescape_string_for_llm_bug("it\\'s working"),
        "it's working"
    );
}

#[test]
fn test_unescape_double_backslash() {
    assert_eq!(unescape_string_for_llm_bug("path\\\\nname"), "path\nname");
}

#[test]
fn test_unescape_trailing_backslash() {
    assert_eq!(unescape_string_for_llm_bug("end\\"), "end\\");
}

#[test]
fn test_unescape_backslash_not_escape() {
    assert_eq!(unescape_string_for_llm_bug("\\a\\b\\c"), "\\a\\b\\c");
}

// ── Trim pair ───────────────────────────────────────────────────

#[test]
fn test_trim_pair_no_trimming_needed() {
    assert!(trim_pair_if_possible("hello", "world", "hello there").is_none());
}

#[test]
fn test_trim_pair_trimming_helps() {
    let (old, new) = trim_pair_if_possible("  hello  ", "  hi  ", "hello world").unwrap();
    assert_eq!(old, "hello");
    assert_eq!(new, "hi");
}

#[test]
fn test_trim_pair_no_content_match() {
    assert!(trim_pair_if_possible("  xyz  ", "  abc  ", "hello world").is_none());
}

// ── Helpers ─────────────────────────────────────────────────────

#[test]
fn test_find_closest_match_found() {
    let hint = find_closest_match(
        "fn main() {\n    let x = 1;\n}\n",
        "fn main() {\n    let x = 2;\n}",
    );
    assert!(hint.contains("partial match"));
}

#[test]
fn test_find_closest_match_not_found() {
    let hint = find_closest_match("fn main() {}\n", "nonexistent_function()");
    assert!(hint.contains("not found anywhere"));
}

#[test]
fn test_diff_stats() {
    assert_eq!(diff_stats("a\nb\nc\n", "a\nB\nc\n"), " (+1/-1 lines)");
    assert_eq!(diff_stats("a\n", "a\nb\n"), " (+1/-0 lines)");
    assert_eq!(diff_stats("a\nb\n", "a\n"), " (+0/-1 lines)");
    assert_eq!(diff_stats("same\n", "same\n"), "");
}

// ── Regex: NoExpand ($-in-replacement) ─────────────────────────

#[test]
fn test_regex_replace_dollar_in_replacement() {
    // $0 should NOT be expanded as a capture group reference
    let content = "function test(){body}";
    let old = "function test ( ) { body }";
    let new = "function cost(){ $0 }";
    let (result, _) = try_regex_replace(content, old, new).unwrap();
    assert!(
        result.contains("$0"),
        "Literal $0 should be preserved, got: {result}"
    );

    // $HOME should NOT be expanded
    let new2 = "echo $HOME";
    let (result2, _) = try_regex_replace(content, old, new2).unwrap();
    assert!(
        result2.contains("$HOME"),
        "Literal $HOME should be preserved, got: {result2}"
    );

    // $1 should NOT be expanded
    let new3 = "let cost = $1.00";
    let (result3, _) = try_regex_replace(content, old, new3).unwrap();
    assert!(
        result3.contains("$1.00"),
        "Literal $1.00 should be preserved, got: {result3}"
    );
}

// ── escape_regex special chars ─────────────────────────────────

#[test]
fn test_escape_regex_special_chars() {
    assert_eq!(escape_regex(r"\"), r"\\");
    assert_eq!(escape_regex("."), r"\.");
    assert_eq!(escape_regex("$"), r"\$");
    assert_eq!(escape_regex("|"), r"\|");
    assert_eq!(escape_regex("("), r"\(");
    assert_eq!(escape_regex(")"), r"\)");
    assert_eq!(escape_regex("["), r"\[");
    assert_eq!(escape_regex("]"), r"\]");
    assert_eq!(escape_regex("{"), r"\{");
    assert_eq!(escape_regex("}"), r"\}");
    assert_eq!(escape_regex("^"), r"\^");
    assert_eq!(escape_regex("+"), r"\+");
    assert_eq!(escape_regex("*"), r"\*");
    assert_eq!(escape_regex("?"), r"\?");
    // Non-special chars pass through
    assert_eq!(escape_regex("abc"), "abc");
    // Mixed
    assert_eq!(escape_regex("a.b"), r"a\.b");
    assert_eq!(escape_regex("$HOME"), r"\$HOME");
}

// ── Regex trailing newline preservation ─────────────────────────

#[test]
fn test_regex_trailing_newline_both_have() {
    // Content ends with \n, replacement preserves it
    let content = "  func(){body}\n";
    let old = "func ( ) { body }";
    let new = "func(){updated}";
    let (result, _) = try_regex_replace(content, old, new).unwrap();
    assert!(
        result.ends_with('\n'),
        "Should preserve trailing newline, got: {result:?}"
    );
}

#[test]
fn test_regex_trailing_newline_neither_has() {
    // Content does NOT end with \n
    let content = "  func(){body}";
    let old = "func ( ) { body }";
    let new = "func(){updated}";
    let (result, _) = try_regex_replace(content, old, new).unwrap();
    assert!(
        !result.ends_with('\n'),
        "Should NOT add trailing newline, got: {result:?}"
    );
}

#[test]
fn test_regex_trailing_newline_content_has_replacement_adds() {
    // Content ends with \n, regex replace might add extra — should stay single \n
    let content = "  func(){body}\n";
    let old = "func ( ) { body }";
    let new = "func(){updated}\n";
    let (result, _) = try_regex_replace(content, old, new).unwrap();
    assert!(
        result.ends_with('\n'),
        "Should have trailing newline, got: {result:?}"
    );
}

#[test]
fn test_regex_trailing_newline_content_lacks_replacement_adds() {
    // Content does NOT end with \n, but replacement does — should strip
    let content = "  func(){body}";
    let old = "func ( ) { body }";
    let new = "func(){updated}\n";
    let (result, _) = try_regex_replace(content, old, new).unwrap();
    assert!(
        !result.ends_with('\n'),
        "Should strip trailing newline to match original, got: {result:?}"
    );
}
