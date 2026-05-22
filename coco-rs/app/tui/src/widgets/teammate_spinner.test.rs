use super::*;

#[test]
fn test_format_idle_duration() {
    assert_eq!(format_idle_duration(5_000), "5s");
    assert_eq!(format_idle_duration(90_000), "1m");
    assert_eq!(format_idle_duration(300_000), "5m");
}

#[test]
fn test_format_token_count() {
    assert_eq!(format_token_count(500), "500");
    assert_eq!(format_token_count(1500), "1.5k");
    assert_eq!(format_token_count(10000), "10.0k");
}

#[test]
fn test_agent_color_to_ratatui() {
    assert_eq!(agent_color_to_ratatui("red"), ratatui::style::Color::Red);
    assert_eq!(agent_color_to_ratatui("cyan"), ratatui::style::Color::Cyan);
    assert_eq!(
        agent_color_to_ratatui("unknown"),
        ratatui::style::Color::Reset
    );
}
