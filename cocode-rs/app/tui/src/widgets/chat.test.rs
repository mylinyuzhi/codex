use super::*;

#[test]
fn test_chat_widget_empty() {
    let messages: Vec<ChatMessage> = vec![];
    let widget = ChatWidget::new(&messages);

    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without panic
}

#[test]
fn test_chat_widget_with_messages() {
    let messages = vec![
        ChatMessage::user("1", "Hello"),
        ChatMessage::assistant("2", "Hi there!"),
    ];
    let widget = ChatWidget::new(&messages);

    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("You"));
    assert!(content.contains("Hello"));
    assert!(content.contains("Assistant"));
}

#[test]
fn test_format_message_user() {
    let widget = ChatWidget::new(&[]);
    let msg = ChatMessage::user("1", "Test message");
    let lines = widget.format_message(&msg);

    assert!(!lines.is_empty());
    // First line should be role indicator
    let first_line: String = lines[0]
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert!(first_line.contains("You"));
}

#[test]
fn test_format_message_with_thinking() {
    let widget = ChatWidget::new(&[]).show_thinking(true);
    let mut msg = ChatMessage::assistant("1", "Response");
    msg.thinking = Some("I'm thinking about this...".to_string());

    let lines = widget.format_message(&msg);
    let content: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();

    assert!(content.contains("Thinking"));
}

#[test]
fn test_format_duration() {
    assert_eq!(
        ChatWidget::format_duration(Duration::from_millis(500)),
        "500ms"
    );
    assert_eq!(ChatWidget::format_duration(Duration::from_secs(2)), "2.0s");
    assert_eq!(ChatWidget::format_duration(Duration::from_secs(90)), "1.5m");
}

#[test]
fn test_thinking_animation_char() {
    let widget = ChatWidget::new(&[]);
    let char0 = widget.thinking_animation_char();
    assert!(!char0.is_ascii()); // Should be a Unicode spinner char

    let widget = ChatWidget::new(&[]).animation_frame(4);
    let char4 = widget.thinking_animation_char();
    assert_ne!(char0, char4); // Different frames have different chars
}
