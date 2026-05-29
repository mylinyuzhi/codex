//! Tests for the self-validating [`super::ToolInputSchema`] newtype (v4.2):
//! construction (`from_input_type` / `from_value`), composition-aware root
//! normalization, `schema_omit_properties`, `SchemaError` classification, and
//! the `resolve-http`-off build invariant. `super::` is the `schema` module.

use serde_json::{Value, json};

#[test]
fn from_input_type_closes_schema_and_validates() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct DemoInput {
        name: String,
        count: Option<i64>,
    }
    let schema = super::ToolInputSchema::from_input_type::<DemoInput>();
    let v = schema.as_value();
    assert_eq!(v["type"], json!("object"));
    assert_eq!(
        v["additionalProperties"],
        json!(false),
        "internal schema must be closed"
    );
    assert!(schema.validate(&json!({"name": "x"})).is_ok());
    assert!(
        schema.validate(&json!({"name": "x", "bogus": 1})).is_err(),
        "additionalProperties:false must reject unknown fields"
    );
    assert!(
        schema.validate(&json!({})).is_err(),
        "missing required `name` must be rejected"
    );
}

#[test]
fn from_value_folds_in_missing_type() {
    let s = super::ToolInputSchema::from_value(json!({
        "properties": {"a": {"type": "string"}}
    }))
    .expect("typeless object schema accepted");
    assert_eq!(
        s.as_value()["type"],
        json!("object"),
        "type:object must be folded in"
    );
}

#[test]
fn from_value_rejects_explicit_non_object_roots() {
    assert!(super::ToolInputSchema::from_value(json!({"type": "array"})).is_err());
    assert!(super::ToolInputSchema::from_value(json!({"type": ["object", "null"]})).is_err());
    assert!(matches!(
        super::ToolInputSchema::from_value(json!({"type": "null"})),
        Err(super::SchemaError::RootTypeNull)
    ));
}

#[test]
fn from_value_does_not_fold_composition_root() {
    // A composition root ($ref) must not gain a spurious type:object.
    if let Ok(s) = super::ToolInputSchema::from_value(json!({
        "$ref": "#/$defs/X",
        "$defs": {"X": {"type": "object"}}
    })) {
        assert!(
            s.as_value().get("type").is_none(),
            "composition root must not gain type:object"
        );
    }
}

#[test]
fn from_value_rejects_remote_ref_without_fetch() {
    // Unknown-scheme $ref is rejected by jsonschema's retriever regardless of
    // the resolve-http feature, and never triggers a network request.
    let s = super::ToolInputSchema::from_value(json!({
        "type": "object",
        "properties": {"x": {"$ref": "made-up-scheme://nope"}}
    }));
    assert!(
        s.is_err(),
        "remote/unknown $ref must be rejected as Err, not fetched"
    );
}

#[test]
fn schema_omit_properties_removes_from_properties_and_required() {
    let base = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {"a": {"type": "string"}, "b": {"type": "string"}},
        "required": ["a", "b"]
    });
    let out = super::schema_omit_properties(&base, &["b"]);
    assert!(out["properties"].get("b").is_none());
    assert_eq!(out["required"], json!(["a"]));
    assert!(
        base["properties"].get("b").is_some(),
        "input must be untouched"
    );
}

#[test]
fn schema_omit_properties_drops_empty_required() {
    let base = json!({
        "type": "object",
        "properties": {"a": {"type": "string"}},
        "required": ["a"]
    });
    let out = super::schema_omit_properties(&base, &["a"]);
    assert!(
        out.get("required").is_none(),
        "empty required must be removed"
    );
}

#[test]
fn schema_error_classifies_invalid_arguments() {
    use coco_error::ErrorExt;
    let e = super::ToolInputSchema::from_value(json!({"type": "array"})).unwrap_err();
    assert_eq!(e.status_code(), coco_error::StatusCode::InvalidArguments);
    assert!(!e.is_retryable());
}

/// Build-invariant: jsonschema must stay `default-features = false` so a remote
/// `$ref` is rejected as `Err` (never fetched — SSRF / blocking-fetch guard).
/// Pure build-graph assertion via `cargo metadata`: no network, fails fast.
#[test]
fn jsonschema_resolve_http_stays_off() {
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    assert!(out.status.success(), "cargo metadata failed");
    let md: Value = serde_json::from_slice(&out.stdout).expect("metadata json");
    let node = md["resolve"]["nodes"]
        .as_array()
        .expect("resolve.nodes")
        .iter()
        .find(|n| n["id"].as_str().is_some_and(|s| s.contains("jsonschema@")))
        .expect("jsonschema in resolved graph");
    let feats: Vec<&str> = node["features"]
        .as_array()
        .expect("features array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        !feats.contains(&"resolve-http") && !feats.contains(&"resolve-file"),
        "jsonschema must stay default-features=false (SSRF/blocking-fetch guard); got {feats:?}",
    );
}
