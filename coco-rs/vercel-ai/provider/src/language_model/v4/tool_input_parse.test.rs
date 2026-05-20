use super::*;
use serde_json::json;

#[test]
fn custom_parser_returns_clean_value() {
    let parser = CustomToolInputParseFunction::new(|raw: &str| {
        let v: Value =
            serde_json::from_str(raw).map_err(|e| ToolInputParseError::Parse(e.to_string()))?;
        Ok(ToolInputParseResult::clean(v))
    });
    let result = parser.parse(r#"{"a": 1}"#).unwrap();
    assert_eq!(result.value, json!({"a": 1}));
    assert!(!result.was_repaired);
}

#[test]
fn custom_parser_can_signal_repair() {
    let parser = CustomToolInputParseFunction::new(|_raw: &str| {
        Ok(ToolInputParseResult::repaired(json!({"a": 1})))
    });
    let result = parser.parse(r#"{a: 1}"#).unwrap();
    assert!(result.was_repaired);
}

#[test]
fn custom_parser_can_signal_failure() {
    let parser = CustomToolInputParseFunction::new(|_raw: &str| {
        Err(ToolInputParseError::Repair("nope".into()))
    });
    match parser.parse("garbage") {
        Err(ToolInputParseError::Repair(msg)) => assert_eq!(msg, "nope"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn handle_can_be_held_as_arc_dyn_trait() {
    // Verifies the type alias compiles for `Arc<dyn ToolInputParseFunction>`
    // call sites (which is how `LanguageModelV4CallOptions` stores it).
    let parser: ToolInputParseHandle = Arc::new(CustomToolInputParseFunction::new(|raw: &str| {
        serde_json::from_str(raw)
            .map(ToolInputParseResult::clean)
            .map_err(|e| ToolInputParseError::Parse(e.to_string()))
    }));
    let result = parser.parse(r#"{"x": "y"}"#).unwrap();
    assert_eq!(result.value, json!({"x": "y"}));
}
