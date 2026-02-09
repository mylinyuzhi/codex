use super::*;

#[test]
fn test_sse_decoder_basic() {
    let mut decoder = SSEDecoder::new();

    // No event yet
    assert!(decoder.decode("event: test").is_none());
    assert!(decoder.decode("data: hello").is_none());

    // Empty line triggers emission
    let event = decoder.decode("").unwrap();
    assert_eq!(event.event, Some("test".to_string()));
    assert_eq!(event.data, "hello");
}

#[test]
fn test_sse_decoder_multiline_data() {
    let mut decoder = SSEDecoder::new();

    decoder.decode("data: line1");
    decoder.decode("data: line2");
    decoder.decode("data: line3");

    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "line1\nline2\nline3");
}

#[test]
fn test_sse_decoder_comment() {
    let mut decoder = SSEDecoder::new();

    // Comment should be ignored
    assert!(decoder.decode(": this is a comment").is_none());
    assert!(decoder.decode("data: actual data").is_none());

    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "actual data");
}

#[test]
fn test_sse_decoder_colon_in_value() {
    let mut decoder = SSEDecoder::new();

    decoder.decode("data: {\"key\": \"value\"}");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "{\"key\": \"value\"}");
}

#[test]
fn test_sse_decoder_no_space_after_colon() {
    let mut decoder = SSEDecoder::new();

    decoder.decode("data:no space");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "no space");
}

#[test]
fn test_sse_decoder_retry() {
    let mut decoder = SSEDecoder::new();

    decoder.decode("retry: 5000");
    decoder.decode("data: test");

    let event = decoder.decode("").unwrap();
    assert_eq!(event.retry, Some(5000));
}

#[test]
fn test_sse_decoder_id() {
    let mut decoder = SSEDecoder::new();

    decoder.decode("id: event-123");
    decoder.decode("data: test");

    let event = decoder.decode("").unwrap();
    assert_eq!(event.id, Some("event-123".to_string()));

    // ID persists per SSE spec
    decoder.decode("data: test2");
    let event2 = decoder.decode("").unwrap();
    assert_eq!(event2.id, Some("event-123".to_string()));
}

#[test]
fn test_sse_decoder_chunk() {
    let mut decoder = SSEDecoder::new();
    let mut buffer = Vec::new();

    let chunk = b"event: test\ndata: hello\n\n";
    let events = decoder.decode_chunk(chunk, &mut buffer);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, Some("test".to_string()));
    assert_eq!(events[0].data, "hello");
}

#[test]
fn test_sse_decoder_partial_chunk() {
    let mut decoder = SSEDecoder::new();
    let mut buffer = Vec::new();

    // First chunk is partial
    let events1 = decoder.decode_chunk(b"event: te", &mut buffer);
    assert!(events1.is_empty());

    // Second chunk completes the event
    let events2 = decoder.decode_chunk(b"st\ndata: hello\n\n", &mut buffer);
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].event, Some("test".to_string()));
    assert_eq!(events2[0].data, "hello");
}

#[test]
fn test_server_sent_event_json() {
    let event = ServerSentEvent {
        event: None,
        data: r#"{"type": "response.output_text.delta", "sequence_number": 1, "item_id": "x", "output_index": 0, "content_index": 0, "delta": "hi", "logprobs": []}"#.to_string(),
        id: None,
        retry: None,
    };

    let parsed: ResponseStreamEvent = event.json().unwrap();
    assert!(matches!(
        parsed,
        ResponseStreamEvent::OutputTextDelta { .. }
    ));
}

#[test]
fn test_map_stream_error() {
    assert!(matches!(
        map_stream_error(Some("context_length_exceeded"), "test"),
        OpenAIError::ContextWindowExceeded
    ));

    assert!(matches!(
        map_stream_error(Some("insufficient_quota"), "test"),
        OpenAIError::QuotaExceeded
    ));

    assert!(matches!(
        map_stream_error(Some("rate_limit_exceeded"), "test"),
        OpenAIError::RateLimited { .. }
    ));

    assert!(matches!(
        map_stream_error(Some("unknown_error"), "test message"),
        OpenAIError::Api { message, .. } if message == "test message"
    ));
}
