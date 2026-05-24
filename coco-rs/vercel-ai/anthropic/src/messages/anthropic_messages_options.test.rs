use super::*;
use serde_json::json;
use std::collections::HashMap;
use vercel_ai_provider::ProviderOptions;

fn options_with(map: HashMap<String, HashMap<String, serde_json::Value>>) -> ProviderOptions {
    ProviderOptions(map)
}

/// `raw` is verbatim — every key (typed-known and unknown) appears
/// in the returned map. Patching this into the wire body lets the
/// user's `extra_body` win over typed body writes at the same key.
#[test]
fn raw_map_includes_every_key_verbatim() {
    let mut inner = HashMap::new();
    inner.insert("disableParallelToolUse".into(), json!(true)); // typed-known
    inner.insert("myCustomField".into(), json!("x")); // unknown
    inner.insert("anotherKey".into(), json!(42)); // unknown
    let mut outer = HashMap::new();
    outer.insert("anthropic".into(), inner);
    let provider_options = Some(options_with(outer));

    let (typed, raw) = extract_anthropic_options(&provider_options, "anthropic.messages");
    assert_eq!(typed.disable_parallel_tool_use, Some(true));

    // Every original key — typed-known AND unknown — appears in raw.
    assert!(
        raw.contains_key("disableParallelToolUse"),
        "typed-known keys are NOT filtered out; user owns correctness"
    );
    assert!(raw.contains_key("myCustomField"));
    assert!(raw.contains_key("anotherKey"));
}

#[test]
fn raw_is_empty_when_no_provider_options() {
    let (typed, raw) = extract_anthropic_options(&None, "anthropic.messages");
    assert!(raw.is_empty());
    assert!(typed.thinking.is_none());
}

#[test]
fn raw_merges_canonical_and_custom_namespace() {
    // Canonical "anthropic" + custom "my-proxy" namespace each
    // contribute keys to `raw`. Custom wins per-key on conflict;
    // both contribute to typed parsing too.
    let mut canonical = HashMap::new();
    canonical.insert("xCanonical".into(), json!("from-canonical"));
    canonical.insert("shared".into(), json!("canonical-value"));
    let mut custom = HashMap::new();
    custom.insert("xCustom".into(), json!("from-custom"));
    custom.insert("shared".into(), json!("custom-value"));
    let mut outer = HashMap::new();
    outer.insert("anthropic".into(), canonical);
    outer.insert("my-proxy".into(), custom);
    let provider_options = Some(options_with(outer));

    let (_typed, raw) = extract_anthropic_options(&provider_options, "my-proxy.messages");
    assert_eq!(raw.get("xCanonical"), Some(&json!("from-canonical")));
    assert_eq!(raw.get("xCustom"), Some(&json!("from-custom")));
    assert_eq!(
        raw.get("shared"),
        Some(&json!("custom-value")),
        "custom namespace wins per-key on conflict"
    );
}
