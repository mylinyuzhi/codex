use super::*;

#[test]
fn test_format_model_short() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();
    let bar = StatusBar::new("gpt-4", &thinking, false, &usage);
    let span = bar.format_model();
    assert!(span.content.contains("gpt-4"));
}

#[test]
fn test_format_thinking_levels() {
    let usage = TokenUsage::default();

    for effort in [
        ReasoningEffort::None,
        ReasoningEffort::Minimal,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::XHigh,
    ] {
        let thinking = ThinkingLevel::new(effort);
        let bar = StatusBar::new("model", &thinking, false, &usage);
        let span = bar.format_thinking();
        assert!(span.content.contains("Think:"));
    }
}

#[test]
fn test_format_plan_mode() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();

    let bar = StatusBar::new("model", &thinking, false, &usage);
    assert!(bar.format_plan_mode().is_none());

    let bar = StatusBar::new("model", &thinking, true, &usage);
    assert!(bar.format_plan_mode().is_some());
}

#[test]
fn test_format_tokens() {
    let thinking = ThinkingLevel::default();

    let usage = TokenUsage::new(500, 200);
    let bar = StatusBar::new("model", &thinking, false, &usage);
    let span = bar.format_tokens();
    assert!(span.content.contains("700"));

    let usage = TokenUsage::new(1500, 500);
    let bar = StatusBar::new("model", &thinking, false, &usage);
    let span = bar.format_tokens();
    assert!(span.content.contains("2.0k"));
}

#[test]
fn test_render() {
    let usage = TokenUsage::new(1000, 500);
    let thinking = ThinkingLevel::new(ReasoningEffort::High);
    let bar = StatusBar::new("claude-sonnet-4", &thinking, true, &usage);

    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    bar.render(area, &mut buf);

    // Check that the buffer contains expected content
    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("claude-sonnet-4"));
    assert!(content.contains("PLAN"));
}

#[test]
fn test_thinking_duration_display() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();

    // While thinking
    let bar = StatusBar::new("model", &thinking, false, &usage)
        .is_thinking(true)
        .thinking_duration(Some(Duration::from_secs(5)));
    let span = bar.format_thinking_status().unwrap();
    assert!(span.content.contains("thinking"));
    assert!(span.content.contains("5s"));

    // After thinking
    let bar = StatusBar::new("model", &thinking, false, &usage)
        .is_thinking(false)
        .thinking_duration(Some(Duration::from_secs(10)));
    let span = bar.format_thinking_status().unwrap();
    assert!(span.content.contains("thought for 10s"));
}

#[test]
fn test_queue_status_display() {
    let usage = TokenUsage::default();
    let thinking = ThinkingLevel::default();

    // No queued items - should not display
    let bar = StatusBar::new("model", &thinking, false, &usage).queue_counts(0, 0);
    assert!(bar.format_queue_status().is_none());

    // Queued commands (also serve as steering)
    let bar = StatusBar::new("model", &thinking, false, &usage).queue_counts(2, 0);
    let span = bar.format_queue_status().unwrap();
    assert!(span.content.contains("2"));
    assert!(span.content.contains("queued"));

    // More queued commands
    let bar = StatusBar::new("model", &thinking, false, &usage).queue_counts(3, 0);
    let span = bar.format_queue_status().unwrap();
    assert!(span.content.contains("3"));
    assert!(span.content.contains("queued"));
}
