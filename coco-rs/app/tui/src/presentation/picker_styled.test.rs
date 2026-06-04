use ratatui::text::Line;

use super::copy_picker_lines;
use super::export_lines;
use super::global_search_lines;
use super::memory_dialog_lines;
use super::quick_open_lines;
use super::session_browser_lines;
use crate::i18n::locale_test_guard;
use crate::state::CopyPickerCodeBlock;
use crate::state::CopyPickerSelection;
use crate::state::CopyPickerState;
use crate::state::ExportFormat;
use crate::state::ExportState;
use crate::state::GlobalSearchState;
use crate::state::MemoryDialogEntry;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::MemoryDialogState;
use crate::state::QuickOpenState;
use crate::state::SearchResult;
use crate::state::SessionBrowserState;
use crate::state::SessionOption;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;

fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn joined(lines: &[Line<'_>]) -> String {
    lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
}

fn cursor_row<'a>(lines: &'a [Line<'a>], needle: &str) -> String {
    lines
        .iter()
        .map(line_text)
        .find(|t| t.contains(needle))
        .unwrap_or_else(|| panic!("row containing {needle:?} not found"))
}

#[test]
fn export_lines_mark_selected_format_with_cursor() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let state = ExportState {
        formats: vec![
            ExportFormat::Markdown,
            ExportFormat::Json,
            ExportFormat::Text,
        ],
        selected: 1,
    };

    let (title, lines, border) = export_lines(&state, styles, 60);
    let texts: Vec<String> = lines.iter().map(line_text).collect();
    let joined = texts.join("\n");

    assert_eq!(title, " Export Transcript ");
    assert_eq!(border, theme.primary);
    assert!(joined.contains("Select format:"), "{joined}");
    // The reusable select list renders a `❯` cursor on the selected row.
    let json_row = texts
        .iter()
        .find(|t| t.contains("JSON (.json)"))
        .expect("json row");
    assert!(
        json_row.starts_with("❯ "),
        "selected row missing cursor: {json_row}"
    );
    let md_row = texts
        .iter()
        .find(|t| t.contains("Markdown (.md)"))
        .expect("md row");
    assert!(
        md_row.starts_with("  "),
        "unselected row has cursor: {md_row}"
    );
}

#[test]
fn memory_lines_render_tags_cursor_and_empty_state() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);

    let (_, empty, _) = memory_dialog_lines(
        &MemoryDialogState {
            entries: Vec::new(),
            selected: 0,
        },
        styles,
        60,
    );
    assert!(joined(&empty).contains("No memory locations resolved."));

    let state = MemoryDialogState {
        entries: vec![
            MemoryDialogEntry {
                path: "CLAUDE.md".into(),
                label: "Project memory".to_string(),
                scope: MemoryDialogScope::Project,
                row_kind: MemoryDialogRowKind::File {
                    exists: true,
                    read_only: false,
                },
            },
            MemoryDialogEntry {
                path: ".claude/local/CLAUDE.md".into(),
                label: "Local memory".to_string(),
                scope: MemoryDialogScope::ProjectLocal,
                row_kind: MemoryDialogRowKind::File {
                    exists: false,
                    read_only: false,
                },
            },
        ],
        selected: 1,
    };
    let (_, lines, _) = memory_dialog_lines(&state, styles, 60);
    assert!(cursor_row(&lines, "Project memory").starts_with("  "));
    let sel = cursor_row(&lines, "Local memory");
    assert!(sel.starts_with("❯ "), "{sel}");
    assert!(sel.contains("[file:new] [project-local] Local memory"));
}

#[test]
fn quick_open_lines_window_to_budget_with_filter() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let state = QuickOpenState {
        filter: "src".to_string(),
        files: (0..20).map(|i| format!("file-{i}.rs")).collect(),
        selected: 2,
    };
    // Budget 10 → the widget windows the full 20-file list to 10 rows around
    // the selection (no pre-truncation), keeping the focused row visible.
    let (_, lines, _) = quick_open_lines(&state, styles, 10);
    let j = joined(&lines);
    assert!(j.contains("Open: src"));
    assert!(cursor_row(&lines, "file-2.rs").starts_with("❯ "));
    assert!(j.contains("file-9.rs"));
    assert!(
        !j.contains("file-10.rs"),
        "window should cap at the budget: {j}"
    );
}

#[test]
fn quick_open_lines_scroll_keeps_far_selection_cursored() {
    // Regression: a selection past the window must scroll into view, not
    // falsely highlight the last visible row.
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let state = QuickOpenState {
        filter: String::new(),
        files: (0..20).map(|i| format!("file-{i}.rs")).collect(),
        selected: 18,
    };
    let (_, lines, _) = quick_open_lines(&state, styles, 10);
    let j = joined(&lines);
    assert!(cursor_row(&lines, "file-18.rs").starts_with("❯ "), "{j}");
    assert!(
        !j.contains("file-2.rs"),
        "window should have scrolled past the start: {j}"
    );
}

#[test]
fn session_browser_lines_filter_and_empty() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);

    let (_, empty, _) = session_browser_lines(
        &SessionBrowserState {
            sessions: Vec::new(),
            filter: String::new(),
            selected: 0,
        },
        styles,
        60,
    );
    assert!(joined(&empty).contains("No saved sessions"));

    let state = SessionBrowserState {
        sessions: vec![SessionOption {
            id: "s1".to_string(),
            label: "Morning".to_string(),
            message_count: 7,
            created_at: "2026-05-14".to_string(),
        }],
        filter: String::new(),
        selected: 0,
    };
    let (_, lines, _) = session_browser_lines(&state, styles, 60);
    assert!(joined(&lines).contains("Type to filter sessions..."));
    assert!(cursor_row(&lines, "Morning").contains("Morning — 7 msgs — 2026-05-14"));
}

#[test]
fn global_search_lines_cap_results_and_states() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let state = GlobalSearchState {
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
    let (_, lines, _) = global_search_lines(&state, styles, 10);
    let j = joined(&lines);
    assert!(j.contains("Search: needle"));
    assert!(cursor_row(&lines, "src/file_2.rs:2 result 2").starts_with("❯ "));
    assert!(j.contains("src/file_9.rs"));
    assert!(
        !j.contains("src/file_10.rs"),
        "window should cap at the budget: {j}"
    );

    let (_, searching, _) = global_search_lines(
        &GlobalSearchState {
            results: Vec::new(),
            is_searching: true,
            ..state.clone()
        },
        styles,
        10,
    );
    assert!(joined(&searching).contains("Searching..."));
}

#[test]
fn copy_picker_lines_map_enum_selection_to_cursor() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let state = CopyPickerState {
        full_text: "hello\nworld".to_string(),
        code_blocks: vec![CopyPickerCodeBlock {
            code: "let x = 1;".to_string(),
            lang: Some("rust".to_string()),
        }],
        message_age: 0,
        selected: CopyPickerSelection::CodeBlock(0),
    };
    let (_, lines, _) = copy_picker_lines(&state, styles, 60);
    // The focused row is the (only) code block, not Full / Always.
    let block_row = cursor_row(&lines, "let x = 1;");
    assert!(block_row.starts_with("❯ "), "{block_row}");
}
