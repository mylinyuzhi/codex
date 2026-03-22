use super::*;

#[test]
fn test_user_queued_command() {
    let cmd = UserQueuedCommand::new("test command");
    assert_eq!(cmd.prompt, "test command");
    assert!(!cmd.id.is_empty());
    assert!(cmd.queued_at > 0);
}

#[test]
fn test_command_preview() {
    let cmd = UserQueuedCommand::new("this is a very long command that should be truncated");
    let preview = cmd.preview(20);
    assert_eq!(preview, "this is a very long ...");

    let short_cmd = UserQueuedCommand::new("short");
    assert_eq!(short_cmd.preview(20), "short");
}

#[test]
fn test_serde_roundtrip() {
    let cmd = UserQueuedCommand::new("test");
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: UserQueuedCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.prompt, cmd.prompt);
}
