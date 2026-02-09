use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn make_ctx_with_extension<T: Send + Sync + 'static>(
    key: &str,
    value: T,
) -> GeneratorContext<'static> {
    let config = Box::leak(Box::new(test_config()));
    GeneratorContext::builder()
        .config(config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .extension(key, value)
        .build()
}

#[tokio::test]
async fn test_async_hook_response_empty() {
    let ctx =
        make_ctx_with_extension::<Vec<AsyncHookResponseInfo>>(ASYNC_HOOK_RESPONSES_KEY, vec![]);
    let generator = AsyncHookResponseGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_async_hook_response_with_data() {
    let responses = vec![AsyncHookResponseInfo {
        hook_name: "test-hook".to_string(),
        additional_context: Some("Test context".to_string()),
        was_blocking: false,
        blocking_reason: None,
        duration_ms: 100,
    }];

    let ctx = make_ctx_with_extension(ASYNC_HOOK_RESPONSES_KEY, responses);
    let generator = AsyncHookResponseGenerator;
    let result = generator.generate(&ctx).await.expect("generate");

    assert!(result.is_some());
    let reminder = result.unwrap();
    assert_eq!(reminder.attachment_type, AttachmentType::AsyncHookResponse);
    assert!(reminder.content().unwrap().contains("test-hook"));
    assert!(reminder.content().unwrap().contains("Test context"));
}

#[tokio::test]
async fn test_hook_blocking_generator() {
    let blocking = vec![HookBlockingInfo {
        hook_name: "security-check".to_string(),
        event_type: "pre_tool_use".to_string(),
        tool_name: Some("bash".to_string()),
        reason: "Command not allowed".to_string(),
    }];

    let ctx = make_ctx_with_extension(HOOK_BLOCKING_KEY, blocking);
    let generator = HookBlockingErrorGenerator;
    let result = generator.generate(&ctx).await.expect("generate");

    assert!(result.is_some());
    let reminder = result.unwrap();
    assert_eq!(reminder.attachment_type, AttachmentType::HookBlockingError);
    assert!(reminder.content().unwrap().contains("security-check"));
    assert!(reminder.content().unwrap().contains("Command not allowed"));
}

#[tokio::test]
async fn test_hook_context_generator() {
    let contexts = vec![HookContextInfo {
        hook_name: "context-hook".to_string(),
        event_type: "session_start".to_string(),
        tool_name: None,
        additional_context: "Session initialized with defaults".to_string(),
    }];

    let ctx = make_ctx_with_extension(HOOK_CONTEXT_KEY, contexts);
    let generator = HookAdditionalContextGenerator;
    let result = generator.generate(&ctx).await.expect("generate");

    assert!(result.is_some());
    let reminder = result.unwrap();
    assert_eq!(
        reminder.attachment_type,
        AttachmentType::HookAdditionalContext
    );
    assert!(reminder.content().unwrap().contains("context-hook"));
    assert!(reminder.content().unwrap().contains("Session initialized"));
}

#[test]
fn test_generator_names() {
    let gen1 = AsyncHookResponseGenerator;
    let gen2 = HookAdditionalContextGenerator;
    let gen3 = HookBlockingErrorGenerator;

    assert_eq!(gen1.name(), "async_hook_response");
    assert_eq!(gen2.name(), "hook_additional_context");
    assert_eq!(gen3.name(), "hook_blocking_error");
}

#[test]
fn test_generator_tiers() {
    let gen1 = AsyncHookResponseGenerator;
    let gen2 = HookAdditionalContextGenerator;
    let gen3 = HookBlockingErrorGenerator;

    assert_eq!(gen1.tier(), ReminderTier::MainAgentOnly);
    assert_eq!(gen2.tier(), ReminderTier::MainAgentOnly);
    assert_eq!(gen3.tier(), ReminderTier::MainAgentOnly);
}
