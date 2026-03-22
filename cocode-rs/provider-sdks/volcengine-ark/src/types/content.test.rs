use super::*;

#[test]
fn test_input_content_text() {
    let block = InputContentBlock::text("Hello");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
    assert!(json.contains(r#""text":"Hello""#));
}

#[test]
fn test_input_content_image_base64() {
    let block = InputContentBlock::image_base64("data123", ImageMediaType::Png);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"image""#));
    assert!(json.contains(r#""data":"data123""#));
    assert!(json.contains(r#""media_type":"image/png""#));
}

#[test]
fn test_input_content_image_url() {
    let block = InputContentBlock::image_url("https://example.com/image.png");
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"image""#));
    assert!(json.contains(r#""url":"https://example.com/image.png""#));
}

#[test]
fn test_input_content_function_output() {
    let block = InputContentBlock::function_call_output("call-1", r#"{"result": 42}"#, None);
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"function_call_output""#));
    assert!(json.contains(r#""call_id":"call-1""#));
}

#[test]
fn test_output_content_block_helpers() {
    let text = OutputContentBlock::Text {
        text: "Hello".to_string(),
    };
    assert_eq!(text.as_text(), Some("Hello"));
    assert!(text.as_function_call().is_none());

    let func = OutputContentBlock::FunctionCall {
        id: "call-1".to_string(),
        name: "test".to_string(),
        arguments: serde_json::json!({}),
    };
    assert!(func.as_text().is_none());
    assert!(func.as_function_call().is_some());
}
