use super::*;

#[test]
fn test_parse_json_str() {
    let result: Value = parse_json_str(r#"{"key": "value"}"#).unwrap();
    assert_eq!(result["key"], "value");
}

#[test]
fn test_parse_json_secure() {
    let json = r#"{"a": {"b": {"c": 1}}}"#;
    let result: Value = parse_json_secure(json.as_bytes(), 10).unwrap();
    assert_eq!(result["a"]["b"]["c"], 1);
}

#[test]
fn test_parse_json_secure_depth_exceeded() {
    let json = r#"{"a": {"b": {"c": 1}}}"#;
    let result: Result<Value, _> = parse_json_secure(json.as_bytes(), 2);
    assert!(matches!(result, Err(SecureJsonError::DepthExceeded)));
}

#[test]
fn test_parse_json_event_stream() {
    let input = "data: {\"event\": \"test\"}\n\ndata: {\"event\": \"test2\"}\n\n";
    assert_eq!(parse_json_event_stream(input.as_bytes()).count(), 2);
}
