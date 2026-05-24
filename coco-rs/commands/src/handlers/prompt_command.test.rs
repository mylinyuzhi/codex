use super::*;
use crate::CommandHandler;
use crate::CommandResult;
use crate::PromptPart;

fn extract_text(result: CommandResult) -> String {
    match result {
        CommandResult::Prompt { parts, .. } => parts
            .into_iter()
            .map(|p| match p {
                PromptPart::Text { text } => text,
                PromptPart::File { .. } => String::new(),
            })
            .collect::<Vec<_>>()
            .join(""),
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[tokio::test]
async fn static_prompt_returns_body_verbatim_with_no_args() {
    let h = StaticPromptHandler::new("test", "running", "BODY");
    let r = h.execute_command("").await.unwrap();
    assert_eq!(extract_text(r), "BODY");
}

#[tokio::test]
async fn static_prompt_with_task_append_appends_args() {
    let h = StaticPromptHandler::with_task_append("test", "running", "BODY");
    let r = h.execute_command("hello world").await.unwrap();
    assert_eq!(extract_text(r), "BODY\n\n## Task\n\nhello world");
}

#[tokio::test]
async fn static_prompt_with_task_append_skips_blank_args() {
    let h = StaticPromptHandler::with_task_append("test", "running", "BODY");
    let r = h.execute_command("   ").await.unwrap();
    assert_eq!(extract_text(r), "BODY");
}

#[tokio::test]
async fn shell_expanding_replaces_simple_marker() {
    let h = ShellExpandingPromptHandler::new("test", "running", "before !`echo hello` after");
    let r = h.execute_command("").await.unwrap();
    assert_eq!(extract_text(r), "before hello after");
}

#[tokio::test]
async fn shell_expanding_handles_multiple_markers() {
    let h = ShellExpandingPromptHandler::new("test", "running", "[!`echo a`][!`echo b`]");
    let r = h.execute_command("").await.unwrap();
    assert_eq!(extract_text(r), "[a][b]");
}

#[tokio::test]
async fn shell_expanding_handles_failed_commands_inline() {
    // Use a single failing command (set -e isn't on by default in bash -c).
    let h =
        ShellExpandingPromptHandler::new("test", "running", "!`bash -c 'echo boom 1>&2; exit 7'`");
    let r = h.execute_command("").await.unwrap();
    let text = extract_text(r);
    assert!(text.starts_with("(error:"), "got: {text:?}");
}

#[tokio::test]
async fn shell_expanding_unterminated_marker_is_preserved() {
    let h = ShellExpandingPromptHandler::new("test", "running", "before !`oops");
    let r = h.execute_command("").await.unwrap();
    assert_eq!(extract_text(r), "before !`oops");
}
