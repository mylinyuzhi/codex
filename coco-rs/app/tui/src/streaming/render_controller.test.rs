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
