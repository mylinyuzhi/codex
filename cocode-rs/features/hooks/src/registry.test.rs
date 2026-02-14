use super::*;
use crate::definition::HookHandler;
use crate::matcher::HookMatcher;
use std::path::PathBuf;

fn make_ctx(event: HookEventType, tool_name: Option<&str>) -> HookContext {
    let mut ctx = HookContext::new(event, "test-session".to_string(), PathBuf::from("/tmp"));
    if let Some(name) = tool_name {
        ctx.tool_name = Some(name.to_string());
    }
    ctx
}

fn make_hook(name: &str, event: HookEventType, matcher: Option<HookMatcher>) -> HookDefinition {
    HookDefinition {
        name: name.to_string(),
        event_type: event,
        matcher,
        handler: HookHandler::Prompt {
            template: "test".to_string(),
            model: None,
        },
        source: Default::default(),
        enabled: true,
        timeout_secs: 30,
        once: false,
    }
}

fn make_once_hook(name: &str, event: HookEventType) -> HookDefinition {
    HookDefinition {
        name: name.to_string(),
        event_type: event,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "test".to_string(),
            model: None,
        },
        source: Default::default(),
        enabled: true,
        timeout_secs: 30,
        once: true,
    }
}

#[test]
fn test_register_and_len() {
    let registry = HookRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    registry.register(make_hook("h1", HookEventType::PreToolUse, None));
    assert!(!registry.is_empty());
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_hooks_for_event() {
    let registry = HookRegistry::new();
    registry.register(make_hook("h1", HookEventType::PreToolUse, None));
    registry.register(make_hook("h2", HookEventType::PostToolUse, None));
    registry.register(make_hook("h3", HookEventType::PreToolUse, None));

    let pre = registry.hooks_for_event(&HookEventType::PreToolUse);
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].name, "h1");
    assert_eq!(pre[1].name, "h3");

    let post = registry.hooks_for_event(&HookEventType::PostToolUse);
    assert_eq!(post.len(), 1);

    let start = registry.hooks_for_event(&HookEventType::SessionStart);
    assert!(start.is_empty());
}

#[test]
fn test_disabled_hooks_excluded() {
    let registry = HookRegistry::new();
    let mut hook = make_hook("disabled", HookEventType::PreToolUse, None);
    hook.enabled = false;
    registry.register(hook);

    assert!(
        registry
            .hooks_for_event(&HookEventType::PreToolUse)
            .is_empty()
    );
}

#[test]
fn test_clear() {
    let registry = HookRegistry::new();
    registry.register(make_hook("h1", HookEventType::PreToolUse, None));
    registry.register(make_hook("h2", HookEventType::PostToolUse, None));
    assert_eq!(registry.len(), 2);

    registry.clear();
    assert!(registry.is_empty());
}

#[tokio::test]
async fn test_execute_no_matching_hooks() {
    let registry = HookRegistry::new();
    let ctx = make_ctx(HookEventType::SessionStart, None);
    let outcomes = registry.execute(&ctx).await;
    assert!(outcomes.is_empty());
}

#[tokio::test]
async fn test_execute_with_matcher() {
    let registry = HookRegistry::new();
    registry.register(make_hook(
        "bash-only",
        HookEventType::PreToolUse,
        Some(HookMatcher::Exact {
            value: "bash".to_string(),
        }),
    ));

    // Should match
    let ctx = make_ctx(HookEventType::PreToolUse, Some("bash"));
    let outcomes = registry.execute(&ctx).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].hook_name, "bash-only");

    // Should not match
    let ctx = make_ctx(HookEventType::PreToolUse, Some("python"));
    let outcomes = registry.execute(&ctx).await;
    assert!(outcomes.is_empty());
}

#[tokio::test]
async fn test_execute_matcher_without_tool_name() {
    let registry = HookRegistry::new();
    registry.register(make_hook(
        "need-tool",
        HookEventType::PreToolUse,
        Some(HookMatcher::All),
    ));

    // No tool name in context but matcher exists => no match
    let ctx = make_ctx(HookEventType::PreToolUse, None);
    let outcomes = registry.execute(&ctx).await;
    assert!(outcomes.is_empty());
}

#[tokio::test]
async fn test_execute_no_matcher_always_matches() {
    let registry = HookRegistry::new();
    registry.register(make_hook("always", HookEventType::SessionStart, None));

    let ctx = make_ctx(HookEventType::SessionStart, None);
    let outcomes = registry.execute(&ctx).await;
    assert_eq!(outcomes.len(), 1);
}

#[tokio::test]
async fn test_once_hook_removed_after_success() {
    let registry = HookRegistry::new();
    registry.register(make_once_hook("one-shot", HookEventType::SessionStart));

    assert_eq!(registry.len(), 1);

    // First execution - hook should run and be removed
    let ctx = make_ctx(HookEventType::SessionStart, None);
    let outcomes = registry.execute(&ctx).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].hook_name, "one-shot");

    // Hook should be removed after successful execution
    assert_eq!(registry.len(), 0);

    // Second execution - no hook should run
    let outcomes = registry.execute(&ctx).await;
    assert!(outcomes.is_empty());
}

#[tokio::test]
async fn test_regular_hook_not_removed() {
    let registry = HookRegistry::new();
    registry.register(make_hook("regular", HookEventType::SessionStart, None));

    assert_eq!(registry.len(), 1);

    let ctx = make_ctx(HookEventType::SessionStart, None);

    // First execution
    let outcomes = registry.execute(&ctx).await;
    assert_eq!(outcomes.len(), 1);

    // Hook should still exist
    assert_eq!(registry.len(), 1);

    // Second execution - hook should still run
    let outcomes = registry.execute(&ctx).await;
    assert_eq!(outcomes.len(), 1);
}

#[test]
fn test_remove_hooks_by_source_name() {
    let registry = HookRegistry::new();
    let mut h1 = make_hook("h1", HookEventType::PreToolUse, None);
    h1.source = crate::scope::HookSource::Skill {
        name: "my-skill".to_string(),
    };
    let mut h2 = make_hook("h2", HookEventType::PreToolUse, None);
    h2.source = crate::scope::HookSource::Skill {
        name: "other-skill".to_string(),
    };
    let h3 = make_hook("h3", HookEventType::PreToolUse, None); // Session source

    registry.register(h1);
    registry.register(h2);
    registry.register(h3);

    assert_eq!(registry.len(), 3);

    registry.remove_hooks_by_source_name("my-skill");

    assert_eq!(registry.len(), 2);
    let hooks = registry.all_hooks();
    assert!(hooks.iter().all(|h| h.name != "h1"));
}

#[test]
fn test_remove_hooks_by_scope() {
    let registry = HookRegistry::new();
    let mut h1 = make_hook("h1", HookEventType::PreToolUse, None);
    h1.source = crate::scope::HookSource::Skill {
        name: "skill".to_string(),
    };
    let mut h2 = make_hook("h2", HookEventType::PreToolUse, None);
    h2.source = crate::scope::HookSource::Policy;
    let h3 = make_hook("h3", HookEventType::PreToolUse, None); // Session source

    registry.register(h1);
    registry.register(h2);
    registry.register(h3);

    assert_eq!(registry.len(), 3);

    registry.remove_hooks_by_scope(crate::scope::HookScope::Session);

    assert_eq!(registry.len(), 2);
    let hooks = registry.all_hooks();
    assert!(hooks.iter().all(|h| h.name != "h3"));
}
