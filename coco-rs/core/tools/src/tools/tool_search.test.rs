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

// ── render_for_model — TS parity for ToolSearch envelopes ─────────────

mod render_tests {
    use super::super::ToolSearchTool;
    use coco_tool_runtime::Tool;
    use coco_tool_runtime::ToolResultContentPart;
    use serde_json::json;

    #[test]
    fn matches_emits_text_list() {
        let data = json!({
            "matches": ["Read", "Grep"],
            "query": "file",
            "total_deferred_tools": 12,
        });
        let parts = ToolSearchTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.starts_with("Matched tools:"), "got: {text}");
        assert!(text.contains("Read"), "got: {text}");
        assert!(text.contains("Grep"), "got: {text}");
    }

    #[test]
    fn empty_matches_without_pending_uses_bare_message() {
        // TS `ToolSearchTool.ts:449`: `'No matching deferred tools found'`
        // (no trailing period).
        let data = json!({
            "matches": [],
            "query": "missing",
            "total_deferred_tools": 0,
        });
        let parts = ToolSearchTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert_eq!(text, "No matching deferred tools found");
    }

    #[test]
    fn empty_matches_with_pending_appends_retry_hint() {
        // TS `ToolSearchTool.ts:454` appends a `. Some MCP servers ...`
        // suffix when servers are still in handshake. The list is
        // joined with `, ` and the suffix ends with a period.
        let data = json!({
            "matches": [],
            "query": "missing",
            "total_deferred_tools": 0,
            "pending_mcp_servers": ["server-a", "server-b"],
        });
        let parts = ToolSearchTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(
            text.starts_with("No matching deferred tools found. Some MCP servers are still connecting: server-a, server-b."),
            "got: {text}"
        );
        assert!(text.ends_with("try searching again."), "got: {text}");
    }

    #[test]
    fn empty_matches_with_empty_pending_array_omits_suffix() {
        let data = json!({
            "matches": [],
            "query": "missing",
            "total_deferred_tools": 0,
            "pending_mcp_servers": [],
        });
        let parts = ToolSearchTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert_eq!(text, "No matching deferred tools found");
    }
}
