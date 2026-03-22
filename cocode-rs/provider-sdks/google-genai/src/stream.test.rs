use super::*;
use bytes::Bytes;
use futures::StreamExt;
use futures::stream;

// =========================================================================
// ServerSentEvent Tests
// =========================================================================

#[test]
fn test_sse_new() {
    let sse = ServerSentEvent::new();
    assert!(sse.event.is_none());
    assert!(sse.data.is_empty());
    assert!(sse.id.is_none());
    assert!(sse.retry.is_none());
}

#[test]
fn test_sse_with_data() {
    let sse = ServerSentEvent::with_data("hello");
    assert_eq!(sse.data, "hello");
    assert!(sse.has_data());
}

#[test]
fn test_sse_is_done() {
    let sse = ServerSentEvent::with_data("[DONE]");
    assert!(sse.is_done());

    let sse2 = ServerSentEvent::with_data("hello");
    assert!(!sse2.is_done());
}

#[test]
fn test_sse_json() {
    let sse = ServerSentEvent::with_data(r#"{"key": "value"}"#);
    let parsed: serde_json::Value = sse.json().unwrap();
    assert_eq!(parsed["key"], "value");
}

// =========================================================================
// SSEDecoder Tests
// =========================================================================

#[test]
fn test_decoder_basic_event() {
    let mut decoder = SSEDecoder::new();
    assert!(decoder.decode("data: hello").is_none());
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "hello");
}

#[test]
fn test_decoder_event_type() {
    let mut decoder = SSEDecoder::new();
    assert!(decoder.decode("event: message").is_none());
    assert!(decoder.decode("data: hello").is_none());
    let event = decoder.decode("").unwrap();
    assert_eq!(event.event, Some("message".to_string()));
    assert_eq!(event.data, "hello");
}

#[test]
fn test_decoder_multiline_data() {
    let mut decoder = SSEDecoder::new();
    assert!(decoder.decode("data: line1").is_none());
    assert!(decoder.decode("data: line2").is_none());
    assert!(decoder.decode("data: line3").is_none());
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "line1\nline2\nline3");
}

#[test]
fn test_decoder_id_field() {
    let mut decoder = SSEDecoder::new();
    assert!(decoder.decode("id: 123").is_none());
    assert!(decoder.decode("data: test").is_none());
    let event = decoder.decode("").unwrap();
    assert_eq!(event.id, Some("123".to_string()));
}

#[test]
fn test_decoder_id_persists() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("id: 123");
    decoder.decode("data: first");
    let event1 = decoder.decode("").unwrap();

    decoder.decode("data: second");
    let event2 = decoder.decode("").unwrap();

    // ID persists across events per SSE spec
    assert_eq!(event1.id, Some("123".to_string()));
    assert_eq!(event2.id, Some("123".to_string()));
}

#[test]
fn test_decoder_id_with_null_ignored() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("id: has\0null");
    decoder.decode("data: test");
    let event = decoder.decode("").unwrap();
    // ID with null character is ignored per SSE spec
    assert!(event.id.is_none());
}

#[test]
fn test_decoder_retry_field() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("retry: 5000");
    decoder.decode("data: test");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.retry, Some(5000));
}

#[test]
fn test_decoder_retry_invalid_ignored() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("retry: invalid");
    decoder.decode("data: test");
    let event = decoder.decode("").unwrap();
    assert!(event.retry.is_none());
}

#[test]
fn test_decoder_comment_ignored() {
    let mut decoder = SSEDecoder::new();
    assert!(decoder.decode(": this is a comment").is_none());
    decoder.decode("data: test");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "test");
}

#[test]
fn test_decoder_space_after_colon() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("data: hello");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "hello"); // Space stripped
}

#[test]
fn test_decoder_no_space_after_colon() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("data:hello");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "hello");
}

#[test]
fn test_decoder_empty_data() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("data:");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "");
}

#[test]
fn test_decoder_unknown_field_ignored() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("unknown: value");
    decoder.decode("data: test");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, "test");
}

#[test]
fn test_decoder_field_without_colon() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("data");
    let event = decoder.decode("").unwrap();
    assert_eq!(event.data, ""); // Treated as empty data
}

#[test]
fn test_decoder_decode_chunk() {
    let mut decoder = SSEDecoder::new();
    let mut buffer = Vec::new();

    let events = decoder.decode_chunk(b"event: test\ndata: hello\n\n", &mut buffer);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, Some("test".to_string()));
    assert_eq!(events[0].data, "hello");
}

#[test]
fn test_decoder_decode_chunk_partial() {
    let mut decoder = SSEDecoder::new();
    let mut buffer = Vec::new();

    // First chunk is partial
    let events1 = decoder.decode_chunk(b"event: te", &mut buffer);
    assert!(events1.is_empty());

    // Second chunk completes
    let events2 = decoder.decode_chunk(b"st\ndata: hi\n\n", &mut buffer);
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].event, Some("test".to_string()));
    assert_eq!(events2[0].data, "hi");
}

#[test]
fn test_decoder_decode_chunk_crlf() {
    let mut decoder = SSEDecoder::new();
    let mut buffer = Vec::new();

    let events = decoder.decode_chunk(b"data: hello\r\n\r\n", &mut buffer);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn test_decoder_reset() {
    let mut decoder = SSEDecoder::new();
    decoder.decode("event: test");
    decoder.decode("data: hello");
    decoder.decode("id: 123");

    decoder.reset();

    decoder.decode("data: new");
    let event = decoder.decode("").unwrap();
    assert!(event.event.is_none());
    assert_eq!(event.data, "new");
    assert!(event.id.is_none());
}

// =========================================================================
// Integration Tests
// =========================================================================

#[tokio::test]
async fn test_parse_sse_events_basic() {
    let data = b"event: message\ndata: hello\n\n";
    let byte_stream = stream::iter(vec![Ok(Bytes::from(&data[..]))]);
    let mut event_stream = parse_sse_events(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    assert_eq!(event.event, Some("message".to_string()));
    assert_eq!(event.data, "hello");
}

#[tokio::test]
async fn test_parse_sse_stream_single_event() {
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]}}]}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    let response = content_stream.next().await.unwrap().unwrap();
    assert_eq!(response.text(), Some("Hello".to_string()));
}

#[tokio::test]
async fn test_parse_sse_stream_multiple_events() {
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]}}]}

data: {"candidates":[{"content":{"role":"model","parts":[{"text":" World"}]}}]}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let content_stream = parse_sse_stream(byte_stream);

    let responses: Vec<_> = content_stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].text(), Some("Hello".to_string()));
    assert_eq!(responses[1].text(), Some(" World".to_string()));
}

#[tokio::test]
async fn test_parse_sse_stream_chunked_delivery() {
    // Simulate data arriving in chunks
    let chunks = vec![
        Ok(Bytes::from("data: {\"candi")),
        Ok(Bytes::from(
            "dates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hi\"}]}}]}\n\n",
        )),
    ];
    let byte_stream = stream::iter(chunks);
    let mut content_stream = parse_sse_stream(byte_stream);

    let response = content_stream.next().await.unwrap().unwrap();
    assert_eq!(response.text(), Some("Hi".to_string()));
}

#[tokio::test]
async fn test_parse_sse_stream_with_done_marker() {
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Done"}]}}]}

data: [DONE]

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let content_stream = parse_sse_stream(byte_stream);

    let responses: Vec<_> = content_stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    // [DONE] marker should not produce a response
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].text(), Some("Done".to_string()));
}

#[tokio::test]
async fn test_parse_sse_stream_with_comments() {
    let data = r#": this is a comment
data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]}}]}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    let response = content_stream.next().await.unwrap().unwrap();
    assert_eq!(response.text(), Some("Hi".to_string()));
}

#[tokio::test]
async fn test_parse_sse_stream_parse_error() {
    // Test with invalid JSON in SSE data
    let data = "data: {invalid json}\n\n";
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    let result = content_stream.next().await.unwrap();
    assert!(result.is_err());
}

// =========================================================================
// Error Handling Tests (Aligned with Python SDK)
// =========================================================================

#[tokio::test]
async fn test_parse_sse_stream_with_error_response() {
    // Aligns with Python SDK test: test_error_event_in_generate_content_stream
    // First chunk is valid, second chunk is an error
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"test"}]}}]}

data: {"error":{"code":500,"message":"Internal Server Error","status":"INTERNAL"}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    // First chunk should succeed
    let response = content_stream.next().await.unwrap().unwrap();
    assert_eq!(response.text(), Some("test".to_string()));

    // Second chunk should be an API error
    let result = content_stream.next().await.unwrap();
    assert!(result.is_err());

    match result.unwrap_err() {
        GenAiError::Api {
            code,
            message,
            status,
        } => {
            assert_eq!(code, 500);
            assert_eq!(message, "Internal Server Error");
            assert_eq!(status, "INTERNAL");
        }
        other => panic!("Expected GenAiError::Api, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_parse_sse_stream_error_only() {
    // Test stream that only returns an error
    let data = r#"data: {"error":{"code":429,"message":"Rate limit exceeded","status":"RESOURCE_EXHAUSTED"}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    let result = content_stream.next().await.unwrap();
    assert!(result.is_err());

    match result.unwrap_err() {
        GenAiError::Api {
            code,
            message,
            status,
        } => {
            assert_eq!(code, 429);
            assert_eq!(message, "Rate limit exceeded");
            assert_eq!(status, "RESOURCE_EXHAUSTED");
        }
        other => panic!("Expected GenAiError::Api, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_parse_sse_stream_error_with_bad_json() {
    // Aligns with Python SDK test: test_error_event_in_streamed_responses_bad_json
    // First chunk valid, second chunk has malformed JSON (not a valid error or response)
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"test"}]}}]}

data: {"error": bad_json}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut content_stream = parse_sse_stream(byte_stream);

    // First chunk should succeed
    let response = content_stream.next().await.unwrap().unwrap();
    assert_eq!(response.text(), Some("test".to_string()));

    // Second chunk should be a parse error (not an API error, since JSON is invalid)
    let result = content_stream.next().await.unwrap();
    assert!(result.is_err());

    match result.unwrap_err() {
        GenAiError::Parse(_) => {} // Expected
        other => panic!("Expected GenAiError::Parse, got: {:?}", other),
    }
}
