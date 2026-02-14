use super::*;
use crate::theme::Theme;

#[test]
fn test_input_widget_empty() {
    let input = InputState::default();
    let theme = Theme::default();
    let widget = InputWidget::new(&input, &theme);

    let area = Rect::new(0, 0, 40, 3);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without panic
}

#[test]
fn test_input_widget_with_text() {
    let mut input = InputState::default();
    input.set_text("Hello");
    let theme = Theme::default();
    let widget = InputWidget::new(&input, &theme);

    let area = Rect::new(0, 0, 40, 3);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("Hello"));
}

#[test]
fn test_input_widget_placeholder() {
    let input = InputState::default();
    let theme = Theme::default();
    let widget = InputWidget::new(&input, &theme).placeholder("Type a message...");

    let area = Rect::new(0, 0, 40, 3);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("Type a message"));
}

#[test]
fn test_input_widget_unfocused() {
    let input = InputState::default();
    let theme = Theme::default();
    let widget = InputWidget::new(&input, &theme).focused(false);

    let area = Rect::new(0, 0, 40, 3);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without cursor blinking
}

#[test]
fn test_get_lines_with_cursor() {
    let mut input = InputState::default();
    input.set_text("Hello");
    input.cursor = 2; // After "He"

    let theme = Theme::default();
    let widget = InputWidget::new(&input, &theme);
    let lines = widget.get_lines();

    assert!(!lines.is_empty());
    // Should have cursor in the middle
    let spans: Vec<_> = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(spans.contains(&"He"));
}

#[test]
fn test_tokenize_plain_text() {
    let tokens = tokenize("hello world");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::Text);
    assert_eq!(tokens[0].text, "hello world");
}

#[test]
fn test_tokenize_at_mention() {
    let tokens = tokenize("read @src/main.rs please");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].token_type, TokenType::Text);
    assert_eq!(tokens[0].text, "read ");
    assert_eq!(tokens[1].token_type, TokenType::AtMention);
    assert_eq!(tokens[1].text, "@src/main.rs");
    assert_eq!(tokens[2].token_type, TokenType::Text);
    assert_eq!(tokens[2].text, " please");
}

#[test]
fn test_tokenize_slash_command() {
    let tokens = tokenize("/commit now");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].token_type, TokenType::SlashCommand);
    assert_eq!(tokens[0].text, "/commit");
    assert_eq!(tokens[1].token_type, TokenType::Text);
    assert_eq!(tokens[1].text, " now");
}

#[test]
fn test_tokenize_mixed() {
    let tokens = tokenize("/review @src/lib.rs");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].token_type, TokenType::SlashCommand);
    assert_eq!(tokens[0].text, "/review");
    assert_eq!(tokens[1].token_type, TokenType::Text);
    assert_eq!(tokens[1].text, " ");
    assert_eq!(tokens[2].token_type, TokenType::AtMention);
    assert_eq!(tokens[2].text, "@src/lib.rs");
}

#[test]
fn test_tokenize_at_not_at_start() {
    // @ in middle of word should not be a mention
    let tokens = tokenize("email@example.com");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::Text);
}

#[test]
fn test_tokenize_slash_not_at_start() {
    // / in middle of word should not be a command
    let tokens = tokenize("path/to/file");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::Text);
}

#[test]
fn test_tokenize_paste_pill() {
    let tokens = tokenize("[Pasted text #1]");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::PastePill);
    assert_eq!(tokens[0].text, "[Pasted text #1]");
}

#[test]
fn test_tokenize_paste_pill_with_lines() {
    let tokens = tokenize("[Pasted text #1 +420 lines]");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::PastePill);
}

#[test]
fn test_tokenize_image_pill() {
    let tokens = tokenize("[Image #1]");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::PastePill);
}

#[test]
fn test_tokenize_mixed_with_pill() {
    let tokens = tokenize("Please analyze [Pasted text #1] and tell me");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].token_type, TokenType::Text);
    assert_eq!(tokens[0].text, "Please analyze ");
    assert_eq!(tokens[1].token_type, TokenType::PastePill);
    assert_eq!(tokens[1].text, "[Pasted text #1]");
    assert_eq!(tokens[2].token_type, TokenType::Text);
    assert_eq!(tokens[2].text, " and tell me");
}

#[test]
fn test_tokenize_non_pill_brackets() {
    // Regular brackets that aren't paste pills
    let tokens = tokenize("[some other thing]");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, TokenType::Text);
}
