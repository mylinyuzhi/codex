use super::*;
use crate::ProviderType;
use crate::model::ModelRole;

fn sample_context() -> InferenceContext {
    let spec = ModelSpec::new("anthropic", "claude-opus-4");
    let info = ModelInfo {
        slug: "claude-opus-4".to_string(),
        context_window: Some(200000),
        max_output_tokens: Some(16384),
        temperature: Some(1.0),
        default_thinking_level: Some(ThinkingLevel::high()),
        ..Default::default()
    };

    InferenceContext::new(
        "call-123",
        "session-456",
        1,
        spec,
        info,
        AgentKind::Main,
        ExecutionIdentity::main(),
    )
}

#[test]
fn test_new_context() {
    let ctx = sample_context();
    assert_eq!(ctx.call_id, "call-123");
    assert_eq!(ctx.session_id, "session-456");
    assert_eq!(ctx.turn_number, 1);
    assert_eq!(ctx.provider(), "anthropic");
    assert_eq!(ctx.model(), "claude-opus-4");
    assert_eq!(ctx.model_spec.provider_type, ProviderType::Anthropic);
}

#[test]
fn test_model_info_accessors() {
    let ctx = sample_context();
    assert_eq!(ctx.context_window(), Some(200000));
    assert_eq!(ctx.max_output_tokens(), Some(16384));
    assert_eq!(ctx.temperature(), Some(1.0));
}

#[test]
fn test_thinking_level() {
    let mut ctx = sample_context();

    // No explicit thinking level, falls back to model default
    assert!(ctx.thinking_level.is_none());
    assert!(ctx.effective_thinking_level().is_some());
    assert_eq!(
        ctx.effective_thinking_level().unwrap().effort,
        ThinkingLevel::high().effort
    );

    // Set explicit thinking level
    ctx = ctx.with_thinking_level(ThinkingLevel::medium());
    assert!(ctx.thinking_level.is_some());
    assert!(ctx.is_thinking_enabled());
    assert_eq!(
        ctx.effective_thinking_level().unwrap().effort,
        ThinkingLevel::medium().effort
    );
}

#[test]
fn test_agent_kind_checks() {
    let ctx = sample_context();
    assert!(ctx.is_main());
    assert!(!ctx.is_subagent());
    assert!(!ctx.is_compaction());
}

#[test]
fn test_child_context() {
    let parent = sample_context();
    let child = parent.child_context(
        "call-child",
        "explore",
        ExecutionIdentity::Role(ModelRole::Explore),
    );

    // Inherits model config
    assert_eq!(child.model_spec, parent.model_spec);
    assert_eq!(child.model_info, parent.model_info);
    assert_eq!(child.session_id, parent.session_id);

    // Has own identity
    assert_eq!(child.call_id, "call-child");
    assert!(child.is_subagent());
    assert_eq!(
        child.original_identity,
        ExecutionIdentity::Role(ModelRole::Explore)
    );
}

#[test]
fn test_request_options() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let ctx = sample_context().with_request_options(opts);

    assert_eq!(ctx.get_request_option("seed"), Some(&serde_json::json!(42)));
    assert_eq!(ctx.get_request_option("nonexistent"), None);
}

#[test]
fn test_serde_roundtrip() {
    let ctx = sample_context().with_thinking_level(ThinkingLevel::high());

    let json = serde_json::to_string(&ctx).unwrap();
    let parsed: InferenceContext = serde_json::from_str(&json).unwrap();

    assert_eq!(ctx.call_id, parsed.call_id);
    assert_eq!(ctx.session_id, parsed.session_id);
    assert_eq!(ctx.model_spec, parsed.model_spec);
    assert_eq!(ctx.thinking_level, parsed.thinking_level);
}
