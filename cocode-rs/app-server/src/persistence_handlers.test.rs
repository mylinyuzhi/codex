use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_extract_nested_value_top_level() {
    let root = serde_json::json!({"model": "gpt-5", "features": {"web_search": true}});
    let result = extract_nested_value(&root, "model");
    assert_eq!(result, serde_json::json!("gpt-5"));
}

#[test]
fn test_extract_nested_value_deep() {
    let root = serde_json::json!({"features": {"web_search": true}});
    let result = extract_nested_value(&root, "features.web_search");
    assert_eq!(result, serde_json::json!(true));
}

#[test]
fn test_extract_nested_value_missing() {
    let root = serde_json::json!({"model": "gpt-5"});
    let result = extract_nested_value(&root, "nonexistent.key");
    assert_eq!(result, serde_json::Value::Null);
}

#[test]
fn test_set_nested_value_top_level() {
    let mut root = serde_json::json!({});
    set_nested_value(&mut root, "model", serde_json::json!("gpt-5"));
    assert_eq!(root, serde_json::json!({"model": "gpt-5"}));
}

#[test]
fn test_set_nested_value_deep() {
    let mut root = serde_json::json!({});
    set_nested_value(&mut root, "features.web_search", serde_json::json!(true));
    assert_eq!(root, serde_json::json!({"features": {"web_search": true}}));
}

#[test]
fn test_set_nested_value_overwrites_existing() {
    let mut root = serde_json::json!({"features": {"web_search": false}});
    set_nested_value(&mut root, "features.web_search", serde_json::json!(true));
    assert_eq!(root, serde_json::json!({"features": {"web_search": true}}));
}

#[test]
fn test_set_nested_value_preserves_siblings() {
    let mut root = serde_json::json!({"features": {"web_search": true, "auto_memory": false}});
    set_nested_value(&mut root, "features.auto_memory", serde_json::json!(true));
    assert_eq!(
        root,
        serde_json::json!({"features": {"web_search": true, "auto_memory": true}})
    );
}
