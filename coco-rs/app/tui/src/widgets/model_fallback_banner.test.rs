use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::ModelFallbackBanner;
use crate::theme::Theme;

#[test]
fn should_display_gates_on_non_empty_description() {
    assert!(!ModelFallbackBanner::should_display(None));
    assert!(!ModelFallbackBanner::should_display(Some("")));
    assert!(ModelFallbackBanner::should_display(Some(
        "opus-4-7 → sonnet-4-6"
    )));
}

#[test]
fn renders_arrow_and_description() {
    let theme = Theme::default();
    let desc = "opus-4-7 → sonnet-4-6";
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            let banner = ModelFallbackBanner::new(desc, &theme);
            frame.render_widget(banner, area);
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Model fallback"));
    assert!(rendered.contains("opus-4-7"));
    assert!(rendered.contains("sonnet-4-6"));
}
