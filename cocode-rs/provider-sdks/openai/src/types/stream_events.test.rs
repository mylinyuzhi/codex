use super::*;

#[test]
fn test_parse_output_text_delta() {
    let json = r#"{
        "type": "response.output_text.delta",
        "sequence_number": 5,
        "item_id": "item-123",
        "output_index": 0,
        "content_index": 0,
        "delta": "Hello",
        "logprobs": []
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        &event,
        ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "Hello"
    ));
    assert_eq!(event.sequence_number(), 5);
    assert_eq!(event.event_type(), "response.output_text.delta");
}

#[test]
fn test_parse_response_completed() {
    let json = r#"{
        "type": "response.completed",
        "sequence_number": 10,
        "response": {
            "id": "resp-123",
            "status": "completed",
            "output": [],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        }
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        event,
        ResponseStreamEvent::ResponseCompleted { .. }
    ));
    assert!(event.is_terminal());
}

#[test]
fn test_parse_function_call_delta() {
    let json = r#"{
        "type": "response.function_call_arguments.delta",
        "sequence_number": 3,
        "item_id": "item-456",
        "output_index": 0,
        "delta": "{\"foo\":"
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        &event,
        ResponseStreamEvent::FunctionCallArgumentsDelta { delta, .. } if delta == "{\"foo\":"
    ));
}

#[test]
fn test_parse_error_event() {
    let json = r#"{
        "type": "error",
        "sequence_number": 1,
        "code": "context_length_exceeded",
        "message": "The context length exceeded the limit"
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(event.is_error());
    assert!(matches!(
        &event,
        ResponseStreamEvent::Error { code: Some(c), message, .. }
            if c == "context_length_exceeded" && message.contains("context")
    ));
}

#[test]
fn test_parse_output_item_added() {
    let json = r#"{
        "type": "response.output_item.added",
        "sequence_number": 2,
        "output_index": 0,
        "item": {
            "type": "message",
            "role": "assistant",
            "content": []
        }
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(event, ResponseStreamEvent::OutputItemAdded { .. }));
}
