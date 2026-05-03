//! Tests for the ToolSearch / Brief / Lsp / Config utility tools.
//!
//! NotebookEditTool tests live alongside the implementation in
//! `notebook_edit.test.rs`.

use super::parse_select_query;

// ---------------------------------------------------------------------------
// B3.2: ToolSearch select: syntax
// ---------------------------------------------------------------------------

#[test]
fn test_parse_select_query_basic() {
    assert_eq!(
        parse_select_query("select:Read,Grep"),
        Some(vec!["Read".into(), "Grep".into()])
    );
}

#[test]
fn test_parse_select_query_whitespace_tolerant() {
    assert_eq!(
        parse_select_query("select: Read , Grep , Glob "),
        Some(vec!["Read".into(), "Grep".into(), "Glob".into()])
    );
}

#[test]
fn test_parse_select_query_single_tool() {
    assert_eq!(parse_select_query("select:Bash"), Some(vec!["Bash".into()]));
}

#[test]
fn test_parse_select_query_drops_empty_entries() {
    assert_eq!(
        parse_select_query("select:Read,,Grep, "),
        Some(vec!["Read".into(), "Grep".into()])
    );
}

#[test]
fn test_parse_select_query_not_select_prefix() {
    assert_eq!(parse_select_query("rust async"), None);
    assert_eq!(parse_select_query("selectable"), None);
    assert_eq!(parse_select_query(""), None);
}

#[test]
fn test_parse_select_query_empty_after_prefix() {
    // `select:` with nothing after is still "select mode" but with no
    // tools — the execute path will reject it. 7 chars exactly.
    assert_eq!(parse_select_query("select:"), Some(vec![]));
}

/// TS uses `/^select:(.+)$/i` — the `/i` makes the prefix match
/// case-insensitive. `Select:`, `SELECT:`, `SeLeCt:` all trigger
/// select mode.
#[test]
fn test_parse_select_query_case_insensitive_prefix() {
    assert_eq!(parse_select_query("Select:Read"), Some(vec!["Read".into()]));
    assert_eq!(
        parse_select_query("SELECT:Read,Grep"),
        Some(vec!["Read".into(), "Grep".into()])
    );
    assert_eq!(parse_select_query("SeLeCt:Bash"), Some(vec!["Bash".into()]));
}

/// The tool NAMES after the prefix are NOT lowercased — only the prefix
/// itself is case-insensitive. This matches TS where the tool lookup
/// uses `findToolByName` which does its own case-insensitive match.
#[test]
fn test_parse_select_query_preserves_tool_name_case() {
    assert_eq!(
        parse_select_query("SELECT:MyCustomTool"),
        Some(vec!["MyCustomTool".into()])
    );
}
