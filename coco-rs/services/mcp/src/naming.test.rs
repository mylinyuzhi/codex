use pretty_assertions::assert_eq;

use super::*;

// ── normalize_name_for_mcp ──

#[test]
fn test_normalize_alphanumeric_unchanged() {
    assert_eq!(normalize_name_for_mcp("slack", false), "slack");
    assert_eq!(normalize_name_for_mcp("my-server", false), "my-server");
    assert_eq!(normalize_name_for_mcp("tool_v2", false), "tool_v2");
}

#[test]
fn test_normalize_replaces_special_chars() {
    assert_eq!(normalize_name_for_mcp("my server", false), "my_server");
    assert_eq!(normalize_name_for_mcp("my.tool", false), "my_tool");
    assert_eq!(
        normalize_name_for_mcp("hello@world!2", false),
        "hello_world_2"
    );
}

#[test]
fn test_normalize_claudeai_collapses_underscores() {
    // claude.ai servers collapse consecutive underscores
    assert_eq!(
        normalize_name_for_mcp("claude.ai Gmail", true),
        "claude_ai_Gmail"
    );
    assert_eq!(
        normalize_name_for_mcp("claude.ai  spaced", true),
        "claude_ai_spaced"
    );
}

#[test]
fn test_normalize_claudeai_strips_leading_trailing() {
    assert_eq!(normalize_name_for_mcp(" test ", true), "test");
    assert_eq!(normalize_name_for_mcp("__test__", true), "test");
}

#[test]
fn test_normalize_non_claudeai_preserves_underscores() {
    // Non-claude.ai servers keep underscores as-is
    assert_eq!(normalize_name_for_mcp("a  b", false), "a__b");
}

// ── mcp_tool_id (with normalization) ──

#[test]
fn test_mcp_tool_id_normalizes() {
    // Spaces become underscores
    assert_eq!(
        mcp_tool_id("my server", "send msg"),
        "mcp__my_server__send_msg"
    );
}

#[test]
fn test_mcp_tool_id_simple() {
    assert_eq!(mcp_tool_id("slack", "send"), "mcp__slack__send");
    assert_eq!(mcp_tool_id("github", "create_pr"), "mcp__github__create_pr");
}

// ── mcp_tool_id_raw ──

#[test]
fn test_mcp_tool_id_raw_no_normalization() {
    assert_eq!(
        mcp_tool_id_raw("already_clean", "tool"),
        "mcp__already_clean__tool"
    );
}

// ── mcp_prefix ──

#[test]
fn test_mcp_prefix() {
    assert_eq!(mcp_prefix("slack"), "mcp__slack__");
    assert_eq!(mcp_prefix("my server"), "mcp__my_server__");
}

// ── mcp_display_name ──

#[test]
fn test_mcp_display_name() {
    assert_eq!(
        mcp_display_name("mcp__slack__send_message", "slack"),
        "send_message"
    );
}

#[test]
fn test_mcp_display_name_no_prefix() {
    assert_eq!(mcp_display_name("unknown_tool", "slack"), "unknown_tool");
}

// ── parse_mcp_tool_id ──

#[test]
fn test_parse_mcp_tool_id() {
    let (server, tool) = parse_mcp_tool_id("mcp__slack__send").unwrap();
    assert_eq!(server, "slack");
    assert_eq!(tool, "send");
}

#[test]
fn test_parse_non_mcp_id() {
    assert!(parse_mcp_tool_id("Read").is_none());
    assert!(parse_mcp_tool_id("mcp__incomplete").is_none());
}

// ── short_request_id ──

#[test]
fn test_short_request_id() {
    assert_eq!(short_request_id("abcdefghij"), "abcdefgh");
    assert_eq!(short_request_id("short"), "short");
}
