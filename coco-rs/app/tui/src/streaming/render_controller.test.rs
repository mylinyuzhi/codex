use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;

use super::*;

#[test]
fn test_stream_render_controller_reuses_stable_prefix_for_new_tail() {
    let theme = Theme::default();
    let mut controller = StreamRenderController::new();

    let first = controller.render(input("first\n\nsecond", &theme));
    let stable_after_first = controller.stable_prefix_end;
    let second = controller.render(input("first\n\nsecond\n\nthird", &theme));

    assert!(stable_after_first > 0);
    assert_eq!(controller.stable_prefix_end, "first\n\nsecond\n\n".len());
    assert!(second.len() >= first.len());
}

#[test]
fn test_stream_render_controller_render_does_not_duplicate_new_stable_lines() {
    let theme = Theme::default();
    let mut controller = StreamRenderController::new();

    let rendered = controller.render(input("alpha\n\nbeta", &theme));
    let text = rendered
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
}

fn input<'a>(source: &'a str, theme: &'a Theme) -> StreamRenderInput<'a> {
    StreamRenderInput {
        source,
        styles: UiStyles::new(theme),
        width: 80,
        syntax_highlighting: SyntaxHighlighting::Disabled,
    }
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn test_stable_lines_remain_prefix_stable_across_advances() {
    // Regression for the loose-list flip (2026-06-10 production
    // `pending_stream_prefix_rows_mismatch` replay): items separated by blank
    // lines arrive incrementally; every already-stable line must survive every
    // later advance byte-identically, because emitted scrollback rows can
    // never be rewritten.
    let theme = Theme::default();
    let source = "Intro paragraph.\n\n- alpha item\n\n- beta item\n\n- gamma item\n\n\
                  Closing paragraph.\n\n### Next section\n\nmore text\n\n";
    let mut controller = StreamRenderController::new();
    let mut prev: Vec<String> = Vec::new();
    let mut fed = 0;
    while fed < source.len() {
        fed = (fed + 7).min(source.len());
        let projection = controller.render_projection(input(&source[..fed], &theme));
        let cur: Vec<String> = projection
            .stable_lines
            .iter()
            .map(|line| format!("{line:?}"))
            .collect();
        assert!(
            cur.len() >= prev.len() && cur[..prev.len()] == prev[..],
            "stable lines must be append-only across advances (fed={fed}):\nwas {prev:#?}\nnow {cur:#?}",
        );
        prev = cur;
    }
}
