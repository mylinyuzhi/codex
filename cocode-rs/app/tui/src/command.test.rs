use super::*;
use cocode_protocol::ReasoningEffort;

#[test]
fn test_user_command_display() {
    let cmd = UserCommand::SubmitInput {
        content: vec![ContentBlock::text("Hello, world!")],
        display_text: "Hello, world!".to_string(),
    };
    assert!(cmd.to_string().contains("SubmitInput"));

    let cmd = UserCommand::Interrupt;
    assert_eq!(cmd.to_string(), "Interrupt");

    let cmd = UserCommand::SetPlanMode { active: true };
    assert!(cmd.to_string().contains("true"));

    let cmd = UserCommand::SetThinkingLevel {
        level: ThinkingLevel::new(ReasoningEffort::High),
    };
    assert!(cmd.to_string().contains("High"));

    let cmd = UserCommand::SetModel {
        model: "claude-sonnet-4".to_string(),
    };
    assert!(cmd.to_string().contains("claude-sonnet-4"));

    let cmd = UserCommand::ApprovalResponse {
        request_id: "req-1".to_string(),
        decision: ApprovalDecision::Approved,
    };
    assert!(cmd.to_string().contains("Approved"));

    let cmd = UserCommand::Shutdown;
    assert_eq!(cmd.to_string(), "Shutdown");
}

#[test]
fn test_long_message_truncation() {
    let long_message = "This is a very long message that should be truncated in display";
    let cmd = UserCommand::SubmitInput {
        content: vec![ContentBlock::text(long_message)],
        display_text: long_message.to_string(),
    };
    let display = cmd.to_string();
    assert!(display.contains("..."));
    assert!(display.len() < long_message.len() + 30);
}

#[test]
fn test_with_correlation_id() {
    let cmd = UserCommand::SubmitInput {
        content: vec![ContentBlock::text("Hello")],
        display_text: "Hello".to_string(),
    };
    let (id1, cmd1) = cmd.with_correlation_id();

    // ID should be a valid UUID (36 chars with hyphens)
    assert_eq!(id1.as_str().len(), 36);

    // Command should be preserved
    if let UserCommand::SubmitInput { display_text, .. } = cmd1 {
        assert_eq!(display_text, "Hello");
    } else {
        panic!("Expected SubmitInput command");
    }

    // Each call should generate unique IDs
    let cmd = UserCommand::Interrupt;
    let (id2, _) = cmd.with_correlation_id();
    assert_ne!(id1.as_str(), id2.as_str());
}

#[test]
fn test_triggers_turn() {
    // Commands that trigger turns
    assert!(
        UserCommand::SubmitInput {
            content: vec![ContentBlock::text("test")],
            display_text: "test".to_string()
        }
        .triggers_turn()
    );
    assert!(
        UserCommand::ExecuteSkill {
            name: "commit".to_string(),
            args: String::new()
        }
        .triggers_turn()
    );
    assert!(
        UserCommand::QueueCommand {
            prompt: "test".to_string()
        }
        .triggers_turn()
    );

    // Commands that don't trigger turns
    assert!(!UserCommand::Interrupt.triggers_turn());
    assert!(!UserCommand::SetPlanMode { active: true }.triggers_turn());
    assert!(
        !UserCommand::SetModel {
            model: "test".to_string()
        }
        .triggers_turn()
    );
    assert!(!UserCommand::Shutdown.triggers_turn());
    assert!(!UserCommand::ClearQueues.triggers_turn());
}
