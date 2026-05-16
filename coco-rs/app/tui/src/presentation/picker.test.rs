use super::*;
use crate::i18n::locale_test_guard;
use crate::state::ExportFormat;
use crate::state::ExportOverlay;
use crate::state::GlobalSearchOverlay;
use crate::state::McpServerOption;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogEntry;
use crate::state::MemoryDialogOverlay;
use crate::state::MemoryDialogScope;
use crate::state::QuickOpenOverlay;
use crate::state::SearchResult;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::theme::Theme;

#[test]
fn grouped_list_inserts_group_headers_and_visible_range() {
    #[derive(Debug)]
    struct Item {
        group: &'static str,
    }

    let items = [
        Item { group: "A" },
        Item { group: "A" },
        Item { group: "B" },
        Item { group: "B" },
    ];
    let refs: Vec<&Item> = items.iter().collect();
    let view = grouped_list(&refs, Some(3), 3, |item| item.group);

    assert!(matches!(view.rows[0], PickerRow::Header("A")));
    assert!(matches!(view.rows[3], PickerRow::Blank));
    assert!(matches!(view.rows[4], PickerRow::Header("B")));
    assert_eq!(view.visible, 4..7);
}

#[test]
fn collapse_hints_keeps_output_within_width() {
    let hints = "Up Down  Left Right  Enter Confirm  Esc Cancel";
    assert_eq!(collapse_hints(hints, 80), hints);

    let collapsed = collapse_hints(hints, 20);
    assert!(collapsed.contains("Up Down"));
    assert!(crate::presentation::layout::text_width(&collapsed) <= 20);

    assert_eq!(collapse_hints(hints, 0), "");
}

#[test]
fn session_browser_content_handles_empty_and_populated_states() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let empty = SessionBrowserOverlay {
        sessions: Vec::new(),
        filter: String::new(),
        selected: 0,
    };

    let (_, empty_body, _) = session_browser_content(&empty, &theme);
    assert_eq!(empty_body, "No saved sessions");

    let populated = SessionBrowserOverlay {
        sessions: vec![SessionOption {
            id: "s1".to_string(),
            label: "Morning".to_string(),
            message_count: 7,
            created_at: "2026-05-14".to_string(),
        }],
        filter: String::new(),
        selected: 0,
    };

    let (title, body, border) = session_browser_content(&populated, &theme);
    assert_eq!(title, " Sessions ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("Type to filter sessions..."));
    assert!(body.contains("▸ Morning — 7 msgs — 2026-05-14"));
    assert!(body.contains("Enter Resume"));
}

#[test]
fn quick_open_content_caps_visible_files_at_fifteen() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let overlay = QuickOpenOverlay {
        filter: "src".to_string(),
        files: (0..20).map(|i| format!("file-{i}.rs")).collect(),
        selected: 2,
    };

    let (title, body, border) = quick_open_content(&overlay, &theme);

    assert_eq!(title, " Quick Open ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("Open: src"));
    assert!(body.contains("▸ file-2.rs"));
    assert!(body.contains("  file-14.rs"));
    assert!(!body.contains("file-15.rs"));
}

#[test]
fn global_search_content_caps_results_and_marks_selection() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let overlay = GlobalSearchOverlay {
        query: "needle".to_string(),
        results: (0..25)
            .map(|i| SearchResult {
                file: format!("src/file_{i}.rs"),
                line_number: i,
                content: format!("  result {i}  "),
            })
            .collect(),
        selected: 2,
        is_searching: false,
    };

    let (title, body, border) = global_search_content(&overlay, &theme);

    assert_eq!(title, " Global Search ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("Search: needle"));
    assert!(body.contains("▸ src/file_2.rs:2 result 2"));
    assert!(body.contains("  src/file_19.rs:19 result 19"));
    assert!(!body.contains("src/file_20.rs"));
}

#[test]
fn global_search_content_reports_searching_and_empty_states() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let searching = GlobalSearchOverlay {
        query: "needle".to_string(),
        results: Vec::new(),
        selected: 0,
        is_searching: true,
    };

    let (_, searching_body, _) = global_search_content(&searching, &theme);
    assert!(searching_body.contains("Searching..."));
    assert!(!searching_body.contains("No results"));

    let empty = GlobalSearchOverlay {
        is_searching: false,
        ..searching
    };
    let (_, empty_body, _) = global_search_content(&empty, &theme);
    assert!(empty_body.contains("No results"));
}

#[test]
fn export_content_marks_selected_format() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let overlay = ExportOverlay {
        formats: vec![
            ExportFormat::Markdown,
            ExportFormat::Json,
            ExportFormat::Text,
        ],
        selected: 1,
    };

    let (title, body, border) = export_content(&overlay, &theme);

    assert_eq!(title, " Export Transcript ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("Select format:"));
    assert!(body.contains("  Markdown (.md)"));
    assert!(body.contains("▸ JSON (.json)"));
    assert!(body.contains("  Plain Text (.txt)"));
}

#[test]
fn memory_dialog_content_renders_scope_tags_and_empty_state() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let empty = MemoryDialogOverlay {
        entries: Vec::new(),
        selected: 0,
    };

    let (title, empty_body, border) = memory_dialog_content(&empty, &theme);
    assert_eq!(title, " Memory ");
    assert_eq!(border, theme.primary);
    assert_eq!(empty_body, "No memory locations resolved.");

    let populated = MemoryDialogOverlay {
        entries: vec![
            MemoryDialogEntry {
                path: "CLAUDE.md".into(),
                label: "Project memory".to_string(),
                scope: MemoryDialogScope::Project,
            },
            MemoryDialogEntry {
                path: ".claude/local/CLAUDE.md".into(),
                label: "Local memory".to_string(),
                scope: MemoryDialogScope::ProjectLocal,
            },
        ],
        selected: 1,
    };

    let (_, body, _) = memory_dialog_content(&populated, &theme);
    assert!(body.contains("Select a memory file to edit:"));
    assert!(body.contains("  [project] Project memory"));
    assert!(body.contains("▸ [project-local] Local memory"));
}

#[test]
fn mcp_server_select_content_preserves_checkbox_rows() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let overlay = McpServerSelectOverlay {
        servers: vec![
            McpServerOption {
                name: "docs".to_string(),
                selected: true,
                tool_count: 2,
            },
            McpServerOption {
                name: "drive".to_string(),
                selected: false,
                tool_count: 1,
            },
        ],
        filter: "d".to_string(),
    };

    let (title, body, border) = mcp_server_select_content(&overlay, &theme);

    assert_eq!(title, " Select MCP Servers ");
    assert_eq!(border, theme.accent);
    assert!(body.contains("Filter: d"));
    assert!(body.contains("  [x] docs (2 tools)"));
    assert!(body.contains("  [ ] drive (1 tools)"));
}
