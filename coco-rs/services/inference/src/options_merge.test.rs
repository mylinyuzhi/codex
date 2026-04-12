use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn test_provider_base_options_anthropic() {
    let opts = provider_base_options("anthropic");
    assert_eq!(
        opts,
        json!({
            "anthropic-beta": ["prompt-caching-2024-07-31"]
        })
    );
}

#[test]
fn test_provider_base_options_openai() {
    let opts = provider_base_options("openai");
    assert_eq!(opts, json!({}));
}

#[test]
fn test_provider_base_options_unknown() {
    let opts = provider_base_options("some-provider");
    assert_eq!(opts, json!({}));
}

#[test]
fn test_merge_provider_options_base_only() {
    let base = json!({"key": "value"});
    let merged = merge_provider_options(&base, None, None);
    assert_eq!(merged, json!({"key": "value"}));
}

#[test]
fn test_merge_provider_options_with_thinking() {
    let base = json!({"key": "value"});
    let thinking = json!({"thinking": true});
    let merged = merge_provider_options(&base, Some(&thinking), None);
    assert_eq!(merged, json!({"key": "value", "thinking": true}));
}

#[test]
fn test_merge_provider_options_with_overrides() {
    let base = json!({"key": "value"});
    let overrides = json!({"key": "overridden", "extra": 42});
    let merged = merge_provider_options(&base, None, Some(&overrides));
    assert_eq!(merged, json!({"key": "overridden", "extra": 42}));
}

#[test]
fn test_merge_provider_options_all_layers() {
    let base = json!({"a": 1, "b": 2});
    let thinking = json!({"b": 3, "c": 4});
    let overrides = json!({"c": 5, "d": 6});
    let merged = merge_provider_options(&base, Some(&thinking), Some(&overrides));
    assert_eq!(merged, json!({"a": 1, "b": 3, "c": 5, "d": 6}));
}

#[test]
fn test_deep_merge_nested_objects() {
    let base = json!({"outer": {"a": 1, "b": 2}});
    let overlay = json!({"outer": {"b": 3, "c": 4}});
    let merged = merge_provider_options(&base, Some(&overlay), None);
    assert_eq!(merged, json!({"outer": {"a": 1, "b": 3, "c": 4}}));
}

#[test]
fn test_deep_merge_array_replaced() {
    let base = json!({"tags": ["a", "b"]});
    let overlay = json!({"tags": ["c"]});
    let merged = merge_provider_options(&base, Some(&overlay), None);
    assert_eq!(merged, json!({"tags": ["c"]}));
}

#[test]
fn test_deep_merge_null_base() {
    let base = json!(null);
    let overlay = json!({"key": "value"});
    let merged = merge_provider_options(&base, Some(&overlay), None);
    assert_eq!(merged, json!({"key": "value"}));
}

#[test]
fn test_deep_merge_empty_objects() {
    let base = json!({});
    let merged = merge_provider_options(&base, None, None);
    assert_eq!(merged, json!({}));
}
