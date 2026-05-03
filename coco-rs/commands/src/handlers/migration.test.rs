use super::*;

#[tokio::test]
async fn ant_user_gets_install_instructions() {
    let cmd = MovedToPluginCommand {
        name: "old".into(),
        description: "old cmd".into(),
        progress_message: "running".into(),
        plugin_name: "myplugin".into(),
        plugin_command: "do-thing".into(),
        user_type: UserType::Ant,
        original_body: "ORIGINAL".into(),
    };
    match cmd.execute_command("").await.unwrap() {
        CommandResult::Prompt { parts, .. } => match &parts[0] {
            PromptPart::Text { text } => {
                assert!(text.contains("claude plugin install myplugin@claude-code-marketplace"));
                assert!(text.contains("/myplugin:do-thing"));
            }
            _ => panic!("expected text part"),
        },
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn human_user_gets_original_prompt() {
    let cmd = MovedToPluginCommand {
        name: "old".into(),
        description: "old cmd".into(),
        progress_message: "running".into(),
        plugin_name: "myplugin".into(),
        plugin_command: "do-thing".into(),
        user_type: UserType::Human,
        original_body: "ORIGINAL".into(),
    };
    match cmd.execute_command("").await.unwrap() {
        CommandResult::Prompt { parts, .. } => match &parts[0] {
            PromptPart::Text { text } => assert_eq!(text, "ORIGINAL"),
            _ => panic!("expected text part"),
        },
        other => panic!("unexpected: {other:?}"),
    }
}
