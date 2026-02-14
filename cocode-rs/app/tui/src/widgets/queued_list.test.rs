use super::*;
use crate::theme::Theme;

fn make_command(prompt: &str) -> UserQueuedCommand {
    UserQueuedCommand {
        id: format!("cmd-{}", prompt.len()),
        prompt: prompt.to_string(),
        queued_at: 1234567890,
    }
}

#[test]
fn test_empty_commands() {
    let theme = Theme::default();
    let commands: Vec<UserQueuedCommand> = vec![];
    let widget = QueuedListWidget::new(&commands, &theme);
    assert_eq!(widget.required_height(), 0);
}

#[test]
fn test_single_command() {
    let theme = Theme::default();
    let commands = vec![make_command("use TypeScript")];
    let widget = QueuedListWidget::new(&commands, &theme);
    assert_eq!(widget.required_height(), 2); // header + 1 command
}

#[test]
fn test_multiple_commands() {
    let theme = Theme::default();
    let commands = vec![
        make_command("use TypeScript"),
        make_command("add error handling"),
        make_command("include tests"),
    ];
    let widget = QueuedListWidget::new(&commands, &theme);
    assert_eq!(widget.required_height(), 4); // header + 3 commands
}

#[test]
fn test_max_display_limit() {
    let theme = Theme::default();
    let commands: Vec<_> = (0..10).map(|i| make_command(&format!("cmd {i}"))).collect();
    let widget = QueuedListWidget::new(&commands, &theme).max_display(3);
    assert_eq!(widget.required_height(), 4); // header + 3 commands (limited)
}

#[test]
fn test_render_empty() {
    let theme = Theme::default();
    let commands: Vec<UserQueuedCommand> = vec![];
    let widget = QueuedListWidget::new(&commands, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without panic, and buffer should be empty
}

#[test]
fn test_render_with_commands() {
    let theme = Theme::default();
    let commands = vec![
        make_command("use TypeScript"),
        make_command("add error handling"),
    ];
    let widget = QueuedListWidget::new(&commands, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without panic
}
