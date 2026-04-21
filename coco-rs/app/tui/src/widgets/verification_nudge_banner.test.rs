use super::VerificationNudgeBanner;
use crate::theme::Theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

#[test]
fn should_display_gates_on_pending_flag() {
    assert!(!VerificationNudgeBanner::should_display(false));
    assert!(VerificationNudgeBanner::should_display(true));
}

#[test]
fn renders_warning_label_and_message() {
    let theme = Theme::default();
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(VerificationNudgeBanner::new(&theme), Rect::new(0, 0, 80, 1));
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let line: String = (0..80)
        .map(|x| buf[(x, 0)].symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(
        line.contains("Verification") || line.contains("⚠"),
        "banner must surface the warning glyph/text: {line:?}"
    );
}
