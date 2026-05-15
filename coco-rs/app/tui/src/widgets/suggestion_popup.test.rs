//! Widget-isolated snapshot tests for `SuggestionPopup`.
//!
//! These render the popup into a small dedicated buffer (no surrounding
//! chat / input chrome), so chrome-layout changes can't break them and
//! the snapshots stay byte-stable across unrelated UI refactors. The
//! full-screen test in `mod.test.rs::test_snapshot_autocomplete_popup`
//! still covers positioning + Z-order; this one covers pure popup
//! rendering.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::SuggestionItem;
use super::SuggestionPopup;
use crate::theme::Theme;

fn item(label: &str, description: Option<&str>) -> SuggestionItem {
    SuggestionItem {
        label: label.to_string(),
        description: description.map(ToString::to_string),
        metadata: None,
    }
}

/// Render the popup as if its "anchor" (the input row) is at the bottom
/// of a `w × h` buffer. The widget computes its own Y by walking up
/// from the anchor, so the popup ends up filling the rows just above.
fn render_popup(items: &[SuggestionItem], selected: usize, w: u16, h: u16) -> String {
    let theme = Theme::default();
    let popup = SuggestionPopup::new(items, &theme).selected(selected);

    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(popup, Rect::new(0, h, w, 1)))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        out.push('\n');
    }
    out
}

#[test]
fn snapshot_short_descriptions() {
    let items = vec![
        item("/clear", Some("Clear chat")),
        item("/config", Some("Settings")),
    ];
    insta::assert_snapshot!("suggestion_popup_short", render_popup(&items, 0, 50, 6));
}

#[test]
fn snapshot_long_description_truncates_within_width() {
    // Verifies that a long description gets truncated with an ellipsis
    // so the row still fits on a single line inside the popup width.
    let items = vec![item(
        "/add-dir",
        Some("<path>  Mount an extra working directory"),
    )];
    insta::assert_snapshot!("suggestion_popup_long_desc", render_popup(&items, 0, 60, 4));
}

#[test]
fn snapshot_cjk_description_reserves_correct_width() {
    // Verifies UnicodeWidthStr is used for sizing so CJK (each char =
    // 2 columns) doesn't underestimate width and clip the right edge.
    let items = vec![item("/帮助", Some("显示帮助信息"))];
    insta::assert_snapshot!("suggestion_popup_cjk", render_popup(&items, 0, 60, 4));
}

#[test]
fn snapshot_selected_row_marker_changes() {
    let items = vec![
        item("/help", Some("Show help")),
        item("/clear", Some("Clear chat")),
        item("/config", Some("Settings")),
    ];
    insta::assert_snapshot!(
        "suggestion_popup_selected_middle",
        render_popup(&items, 1, 50, 6)
    );
}

#[test]
fn snapshot_uniform_name_column_padding() {
    // Verifies all rows share a single padded name column so the
    // descriptions line up vertically, matching TS Claude Code's
    // PromptInputFooterSuggestions layout.
    let items = vec![
        item("/m", Some("model")),
        item("/clear", Some("clear chat")),
        item("/commit-push-pr", Some("commit + push + PR")),
    ];
    insta::assert_snapshot!(
        "suggestion_popup_column_alignment",
        render_popup(&items, 0, 60, 6)
    );
}

#[test]
fn empty_items_renders_nothing() {
    let out = render_popup(&[], 0, 30, 4);
    // Widget early-returns; the buffer stays as default cells (spaces).
    assert!(out.chars().all(|c| c == ' ' || c == '\n'));
}
