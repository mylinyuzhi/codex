use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_cache_safe_params_serde_roundtrip() {
    let params = CacheSafeParams {
        rendered_system_prompt: "You are a helpful agent.".into(),
        model_id: "claude-opus-4-7".into(),
        fork_context_messages: vec![
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({"role": "assistant", "content": "hello"}),
        ],
    };
    let s = serde_json::to_string(&params).unwrap();
    let back: CacheSafeParams = serde_json::from_str(&s).unwrap();
    assert_eq!(params, back);
}

#[test]
fn test_cache_safe_params_default_skips_empty_fork_messages() {
    // `fork_context_messages` defaults to empty on deserialize so
    // future on-disk session formats can omit it without breaking.
    let json = r#"{
        "rendered_system_prompt": "sys",
        "model_id": "m"
    }"#;
    let parsed: CacheSafeParams = serde_json::from_str(json).unwrap();
    assert!(parsed.fork_context_messages.is_empty());
    assert_eq!(parsed.model_id, "m");
}

#[test]
fn test_cache_safe_params_eq_distinguishes_model() {
    let a = CacheSafeParams {
        rendered_system_prompt: "sys".into(),
        model_id: "claude-opus-4-7".into(),
        fork_context_messages: Vec::new(),
    };
    let b = CacheSafeParams {
        model_id: "claude-haiku-4-5".into(),
        ..a.clone()
    };
    assert_ne!(a, b, "different model must compare unequal");
}
