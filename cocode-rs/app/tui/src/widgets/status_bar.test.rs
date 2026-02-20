use super::*;

fn default_theme() -> Theme {
    Theme::default()
}

#[test]
fn test_format_model_short() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();
    let bar = StatusBar::new("gpt-4", &thinking, false, &usage, &theme);
    let span = bar.format_model();
    assert!(span.content.contains("gpt-4"));
}

#[test]
fn test_format_thinking_levels() {
    let usage = TokenUsage::default();
    let theme = default_theme();

    for effort in [
        ReasoningEffort::None,
        ReasoningEffort::Minimal,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::XHigh,
    ] {
        let thinking = ThinkingLevel::new(effort);
        let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
        let span = bar.format_thinking();
        assert!(span.content.contains("Think:"));
    }
}

#[test]
fn test_format_plan_mode() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    assert!(bar.format_plan_mode().is_none());

    let bar = StatusBar::new("model", &thinking, true, &usage, &theme);
    assert!(bar.format_plan_mode().is_some());
}

#[test]
fn test_format_tokens() {
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    let usage = TokenUsage::new(500, 200);
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    let span = bar.format_tokens();
    assert!(span.content.contains("500"));
    assert!(span.content.contains("200"));

    let usage = TokenUsage::new(1500, 500);
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    let span = bar.format_tokens();
    assert!(span.content.contains("1.5k"));
    assert!(span.content.contains("500"));
}

#[test]
fn test_render() {
    let usage = TokenUsage::new(1000, 500);
    let thinking = ThinkingLevel::new(ReasoningEffort::High);
    let theme = default_theme();
    let bar = StatusBar::new("claude-sonnet-4", &thinking, true, &usage, &theme);

    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    bar.render(area, &mut buf);

    // Check that the buffer contains expected content
    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(content.contains("claude-sonnet-4"));
    assert!(content.contains("PLAN"));
}

#[test]
fn test_thinking_duration_display() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    // While thinking
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme)
        .is_thinking(true)
        .thinking_duration(Some(Duration::from_secs(5)));
    let span = bar.format_thinking_status().unwrap();
    assert!(span.content.contains("thinking"));
    assert!(span.content.contains("5s"));

    // After thinking
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme)
        .is_thinking(false)
        .thinking_duration(Some(Duration::from_secs(10)));
    let span = bar.format_thinking_status().unwrap();
    assert!(span.content.contains("thought for 10s"));
}

#[test]
fn test_queue_status_display() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    // No queued items - should not display
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme).queue_counts(0, 0);
    assert!(bar.format_queue_status().is_none());

    // Queued commands
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme).queue_counts(2, 0);
    let span = bar.format_queue_status().unwrap();
    assert!(span.content.contains("2"));
    assert!(span.content.contains("queued"));

    // More queued commands
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme).queue_counts(3, 0);
    let span = bar.format_queue_status().unwrap();
    assert!(span.content.contains("3"));
    assert!(span.content.contains("queued"));
}

#[test]
fn test_context_gauge() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    // No context window data
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    assert!(bar.format_context_gauge().is_none());

    // With context window data
    let bar =
        StatusBar::new("model", &thinking, false, &usage, &theme).context_window(62_000, 100_000);
    let span = bar.format_context_gauge().unwrap();
    assert!(span.content.contains("62%"));
}

#[test]
fn test_cost_estimate() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    // No cost
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    assert!(bar.format_cost().is_none());

    // With cost
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme).estimated_cost(150);
    let span = bar.format_cost().unwrap();
    assert!(span.content.contains("$1.50"));
}

#[test]
fn test_working_dir() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let theme = default_theme();

    // No working directory
    let bar = StatusBar::new("model", &thinking, false, &usage, &theme);
    assert!(bar.format_working_dir().is_none());

    // With working directory
    let bar =
        StatusBar::new("model", &thinking, false, &usage, &theme).working_dir(Some("/tmp/test"));
    let span = bar.format_working_dir().unwrap();
    assert!(span.content.contains("/tmp/test"));
}
