use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::RateLimitPanel;
use super::format_duration;
use crate::state::session::RateLimitInfo;
use crate::theme::Theme;

#[test]
fn should_display_matches_blocking_state_only() {
    assert!(!RateLimitPanel::should_display(None));
    assert!(!RateLimitPanel::should_display(Some(&RateLimitInfo {
        remaining: Some(5),
        reset_at: None,
        provider: None,
    })));
    assert!(RateLimitPanel::should_display(Some(&RateLimitInfo {
        remaining: Some(0),
        reset_at: None,
        provider: None,
    })));
}

#[test]
fn format_duration_branches_cover_all_ranges() {
    assert_eq!(format_duration(0), "now");
    assert_eq!(format_duration(-5), "now");
    assert_eq!(format_duration(45), "45s");
    assert_eq!(format_duration(125), "2m 5s");
    assert_eq!(format_duration(3725), "1h 2m");
}

#[test]
fn renders_reset_countdown_and_provider() {
    let info = RateLimitInfo {
        remaining: Some(0),
        reset_at: Some(1000),
        provider: Some("anthropic".to_string()),
    };
    let theme = Theme::default();
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.area();
            let panel = RateLimitPanel::new(&info, &theme).with_now(400);
            frame.render_widget(panel, area);
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(
        rendered.contains("Rate limited"),
        "expected banner text, got: {rendered}"
    );
    assert!(
        rendered.contains("anthropic"),
        "expected provider tag, got: {rendered}"
    );
    assert!(
        rendered.contains("10m 0s"),
        "expected 10m 0s countdown, got: {rendered}"
    );
}
