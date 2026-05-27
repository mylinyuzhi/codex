use super::*;
use crate::i18n::locale_test_guard;
use crate::presentation::styles::UiStyles;
use crate::state::ExportFormat;
use crate::state::ExportState;
use crate::state::GlobalSearchState;
use crate::state::McpServerOption;
use crate::state::McpServerSelectState;
use crate::state::MemoryDialogEntry;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::MemoryDialogState;
use crate::state::QuickOpenState;
use crate::state::SearchResult;
use crate::state::SessionBrowserState;
use crate::state::SessionOption;
use crate::state::SkillsDialogState;
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
    let empty = SessionBrowserState {
        sessions: Vec::new(),
        filter: String::new(),
        selected: 0,
    };

    let (_, empty_body, _) = session_browser_content(&empty, UiStyles::new(&theme));
    assert_eq!(empty_body, "No saved sessions");

    let populated = SessionBrowserState {
        sessions: vec![SessionOption {
            id: "s1".to_string(),
            label: "Morning".to_string(),
            message_count: 7,
            created_at: "2026-05-14".to_string(),
        }],
        filter: String::new(),
        selected: 0,
    };

    let (title, body, border) = session_browser_content(&populated, UiStyles::new(&theme));
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
    let state = QuickOpenState {
        filter: "src".to_string(),
        files: (0..20).map(|i| format!("file-{i}.rs")).collect(),
        selected: 2,
    };

    let (title, body, border) = quick_open_content(&state, UiStyles::new(&theme));

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

    let (title, body, border) = global_search_content(&state, UiStyles::new(&theme));

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
    let searching = GlobalSearchState {
        query: "needle".to_string(),
        results: Vec::new(),
        selected: 0,
        is_searching: true,
    };

    let (_, searching_body, _) = global_search_content(&searching, UiStyles::new(&theme));
    assert!(searching_body.contains("Searching..."));
    assert!(!searching_body.contains("No results"));

    let empty = GlobalSearchState {
        is_searching: false,
        ..searching
    };
    let (_, empty_body, _) = global_search_content(&empty, UiStyles::new(&theme));
    assert!(empty_body.contains("No results"));
}

#[test]
fn export_content_marks_selected_format() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = ExportState {
        formats: vec![
            ExportFormat::Markdown,
            ExportFormat::Json,
            ExportFormat::Text,
        ],
        selected: 1,
    };

    let (title, body, border) = export_content(&state, UiStyles::new(&theme));

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
    let empty = MemoryDialogState {
        entries: Vec::new(),
        selected: 0,
    };

    let (title, empty_body, border) = memory_dialog_content(&empty, UiStyles::new(&theme));
    assert_eq!(title, " Memory ");
    assert_eq!(border, theme.primary);
    assert_eq!(empty_body, "No memory locations resolved.");

    let populated = MemoryDialogState {
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

    let (_, body, _) = memory_dialog_content(&populated, UiStyles::new(&theme));
    assert!(body.contains("Select a memory file to edit:"));
    assert!(body.contains("  [file:exists] [project] Project memory"));
    assert!(body.contains("▸ [file:new] [project-local] Local memory"));
}

#[test]
fn skills_dialog_content_renders_flat_list_with_state_and_lock() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();

    // Empty catalog → "no skills" hint, border stays primary.
    let empty = SkillsDialogState::from_wire(coco_types::SkillsDialogPayload {
        entries: Vec::new(),
        bytes_per_token: 4,
    });
    let (title, body, border) = skills_dialog_content(&empty, UiStyles::new(&theme));
    assert_eq!(title, " Skills ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("No skills found."));

    // Mixed catalog: free user skill, plugin-locked skill, off-overridden
    // skill — covers 4-state glyph + lock annotation + plugin footer.
    let payload = coco_types::SkillsDialogPayload {
        entries: vec![
            coco_types::SkillsDialogEntry {
                name: "deploy".into(),
                source: coco_types::SkillsDialogSource::Project,
                description: "Run cargo deploy".into(),
                plugin_name: None,
                frontmatter_bytes: 168,
                current_local: None,
                baseline: coco_types::SkillOverrideState::On,
                lock: None,
            },
            coco_types::SkillsDialogEntry {
                name: "claude-api".into(),
                source: coco_types::SkillsDialogSource::Plugin,
                description: "Anthropic SDK helper".into(),
                plugin_name: Some("claude-plugins-official".into()),
                frontmatter_bytes: 120,
                current_local: None,
                baseline: coco_types::SkillOverrideState::On,
                lock: Some(coco_types::SkillLock {
                    source: coco_types::SkillLockSource::Plugin,
                    forced_value: coco_types::SkillOverrideState::On,
                }),
            },
            coco_types::SkillsDialogEntry {
                name: "noisy".into(),
                source: coco_types::SkillsDialogSource::User,
                description: "loud".into(),
                plugin_name: None,
                frontmatter_bytes: 400,
                current_local: Some(coco_types::SkillOverrideState::Off),
                baseline: coco_types::SkillOverrideState::On,
                lock: None,
            },
        ],
        bytes_per_token: 4,
    };
    let state = SkillsDialogState::from_wire(payload);
    let (_, body, _) = skills_dialog_content(&state, UiStyles::new(&theme));

    // Subtitle includes total + hint.
    assert!(body.contains("3 skills"));
    // Filter placeholder.
    assert!(body.contains("Search skills"));
    // Free row shows state + source + token suffix.
    assert!(body.contains("deploy"));
    // Plugin row carries lock annotation in the locked-by suffix.
    assert!(body.contains("claude-api"));
    assert!(body.contains("locked by plugin"));
    // The off-row shows the "off" label (mirrors `rT5`).
    assert!(body.contains("off"));
    // Plugin footer.
    assert!(body.contains("Plugin skills are managed via /plugin"));
}

#[test]
fn mcp_server_select_content_preserves_checkbox_rows() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = McpServerSelectState {
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

    let (title, body, border) = mcp_server_select_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Select MCP Servers ");
    assert_eq!(border, theme.accent);
    assert!(body.contains("Filter: d"));
    assert!(body.contains("  [x] docs (2 tools)"));
    assert!(body.contains("  [ ] drive (1 tools)"));
}
