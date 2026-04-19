//! Tests for session title auto-generation.

use super::*;

#[test]
fn prompt_includes_plan_body() {
    let (_sys, user) = build_title_prompt("# Fix auth\n- add middleware");
    assert!(user.contains("Fix auth"));
    assert!(user.contains("add middleware"));
    assert!(user.contains("--- PLAN ---"));
}

#[test]
fn prompt_truncates_long_plans_safely() {
    let plan = "x".repeat(5_000);
    let (_sys, user) = build_title_prompt(&plan);
    // Truncation cap is PLAN_CONTEXT_CHARS=1000. The body + label
    // overhead is bounded; just verify we didn't dump the full 5k.
    assert!(user.len() < 1500);
}

#[test]
fn prompt_is_char_boundary_safe() {
    // Build a string whose cut-point would split a multi-byte char if
    // we did naive byte slicing. With PLAN_CONTEXT_CHARS=1000, we place
    // a 2-byte emoji right at byte 999 (non-boundary) and 1000 (emoji
    // start) to exercise the backtracking logic.
    let mut plan = "a".repeat(998);
    plan.push('é'); // 2-byte UTF-8
    plan.push_str(&"b".repeat(3000));
    // Must not panic.
    let (_sys, user) = build_title_prompt(&plan);
    assert!(user.contains("--- PLAN ---"));
}

#[test]
fn parse_strict_json_object() {
    assert_eq!(
        parse_title_response(r#"{"title":"Fix login button"}"#),
        Some("Fix login button".into())
    );
}

#[test]
fn parse_json_object_with_whitespace() {
    assert_eq!(
        parse_title_response("  \n { \"title\" : \"Update docs\" }  "),
        Some("Update docs".into())
    );
}

#[test]
fn parse_bare_json_string_fallback() {
    assert_eq!(
        parse_title_response("\"Refactor auth flow\""),
        Some("Refactor auth flow".into())
    );
}

#[test]
fn parse_plaintext_response_fallback() {
    // Model ignored schema — extract first line.
    assert_eq!(
        parse_title_response("Fix the dashboard crash"),
        Some("Fix the dashboard crash".into())
    );
}

#[test]
fn parse_plaintext_strips_title_label() {
    assert_eq!(
        parse_title_response("title: Add caching"),
        Some("Add caching".into())
    );
    assert_eq!(
        parse_title_response("Title: Add caching"),
        Some("Add caching".into())
    );
}

#[test]
fn parse_strips_trailing_punctuation() {
    assert_eq!(
        parse_title_response(r#"{"title":"Fix bug!"}"#),
        Some("Fix bug".into())
    );
    assert_eq!(
        parse_title_response(r#"{"title":"Fix bug."}"#),
        Some("Fix bug".into())
    );
}

#[test]
fn parse_rejects_too_short() {
    assert_eq!(parse_title_response(r#"{"title":"OK"}"#), None);
    assert_eq!(parse_title_response("  "), None);
    assert_eq!(parse_title_response(""), None);
}

#[test]
fn parse_caps_very_long_titles() {
    let long = "x".repeat(500);
    let raw = format!(r#"{{"title":"{long}"}}"#);
    let parsed = parse_title_response(&raw).unwrap();
    assert!(parsed.len() <= 100);
}

#[test]
fn apply_title_persists_when_session_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = crate::SessionManager::new(tmp.path().to_path_buf());
    let s = mgr.create("claude-opus-4-6", tmp.path()).unwrap();
    assert!(s.title.is_none());

    let applied = apply_title(&mgr, &s.id, "Fix login button".into()).unwrap();
    assert!(applied, "expected title to be applied");

    let reloaded = mgr.load(&s.id).unwrap();
    assert_eq!(reloaded.title.as_deref(), Some("Fix login button"));
}

#[test]
fn apply_title_respects_existing_title() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = crate::SessionManager::new(tmp.path().to_path_buf());
    let mut s = mgr.create("claude-opus-4-6", tmp.path()).unwrap();
    s.title = Some("User-set title".into());
    mgr.save(&s).unwrap();

    let applied = apply_title(&mgr, &s.id, "Auto title".into()).unwrap();
    assert!(!applied, "must not overwrite user-set title");

    let reloaded = mgr.load(&s.id).unwrap();
    assert_eq!(reloaded.title.as_deref(), Some("User-set title"));
}
