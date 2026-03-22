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
fn test_parse_response_completed_missing_usage_is_none() {
    let json = r#"{
        "type": "response.completed",
        "sequence_number": 11,
        "response": {
            "id": "resp-123",
            "status": "completed",
            "output": []
        }
    }"#;

    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    if let ResponseStreamEvent::ResponseCompleted { response, .. } = event {
        assert!(response.usage_opt().is_none());
    } else {
        panic!("expected ResponseCompleted event");
    }
}

#[test]
fn test_parse_response_completed_null_usage_is_none() {
    let json = r#"{
        "type": "response.completed",
        "sequence_number": 12,
        "response": {
            "id": "resp-123",
            "status": "completed",
            "output": [],
            "usage": null
        }
    }"#;

    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    if let ResponseStreamEvent::ResponseCompleted { response, .. } = event {
        assert!(response.usage_opt().is_none());
    } else {
        panic!("expected ResponseCompleted event");
    }
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

#[test]
fn test_parse_refusal_delta() {
    let json = r#"{
        "type": "response.refusal.delta",
        "sequence_number": 4,
        "item_id": "item-789",
        "output_index": 0,
        "content_index": 0,
        "delta": "I cannot help with"
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        &event,
        ResponseStreamEvent::RefusalDelta { delta, content_index, .. }
            if delta == "I cannot help with" && *content_index == 0
    ));
    assert_eq!(event.sequence_number(), 4);
    assert_eq!(event.event_type(), "response.refusal.delta");
}

#[test]
fn test_parse_refusal_done() {
    let json = r#"{
        "type": "response.refusal.done",
        "sequence_number": 5,
        "item_id": "item-789",
        "output_index": 0,
        "content_index": 0,
        "refusal": "I cannot help with that request."
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        &event,
        ResponseStreamEvent::RefusalDone { refusal, .. }
            if refusal == "I cannot help with that request."
    ));
    assert_eq!(event.sequence_number(), 5);
    assert_eq!(event.event_type(), "response.refusal.done");
}

#[test]
fn test_parse_response_failed() {
    let json = r#"{
        "type": "response.failed",
        "sequence_number": 8,
        "response": {
            "id": "resp-fail-1",
            "status": "failed",
            "output": [],
            "usage": {"input_tokens": 10, "output_tokens": 0, "total_tokens": 10},
            "error": {
                "code": "server_error",
                "message": "Internal server error occurred"
            }
        }
    }"#;
    let event: ResponseStreamEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        &event,
        ResponseStreamEvent::ResponseFailed { response, .. }
            if response
                .error
                .as_ref()
                .and_then(|e| e.code_opt())
                == Some("server_error")
    ));
    assert!(event.is_terminal());
    assert_eq!(event.event_type(), "response.failed");
}
