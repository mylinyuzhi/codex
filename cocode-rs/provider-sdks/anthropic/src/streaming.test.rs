use super::*;
use futures::stream;

#[test]
fn test_parse_text_delta() {
    let delta: ContentBlockDelta =
        serde_json::from_str(r#"{"type": "text_delta", "text": "Hello"}"#).unwrap();

    assert!(matches!(
        delta,
        ContentBlockDelta::TextDelta { text } if text == "Hello"
    ));
}

#[test]
fn test_parse_input_json_delta() {
    let delta: ContentBlockDelta =
        serde_json::from_str(r#"{"type": "input_json_delta", "partial_json": "{\"key\":"}"#)
            .unwrap();

    assert!(matches!(
        delta,
        ContentBlockDelta::InputJsonDelta { partial_json }
        if partial_json == "{\"key\":"
    ));
}

#[test]
fn test_parse_thinking_delta() {
    let delta: ContentBlockDelta =
        serde_json::from_str(r#"{"type": "thinking_delta", "thinking": "Let me think..."}"#)
            .unwrap();

    assert!(matches!(
        delta,
        ContentBlockDelta::ThinkingDelta { thinking } if thinking == "Let me think..."
    ));
}

#[tokio::test]
async fn test_parse_sse_message_start() {
    let data = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg-123","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut event_stream = parse_sse_stream(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    assert!(matches!(event, RawMessageStreamEvent::MessageStart { .. }));
}

#[tokio::test]
async fn test_parse_sse_content_block_delta() {
    let data = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut event_stream = parse_sse_stream(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    match event {
        RawMessageStreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(index, 0);
            assert!(matches!(
                delta,
                ContentBlockDelta::TextDelta { text } if text == "Hello"
            ));
        }
        _ => panic!("Expected ContentBlockDelta"),
    }
}

#[tokio::test]
async fn test_parse_sse_multiple_events() {
    let data = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg-123","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" World"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let event_stream = parse_sse_stream(byte_stream);
    let events: Vec<_> = event_stream.collect().await;

    assert_eq!(events.len(), 7);
    assert!(matches!(
        events[0].as_ref().unwrap(),
        RawMessageStreamEvent::MessageStart { .. }
    ));
    assert!(matches!(
        events[1].as_ref().unwrap(),
        RawMessageStreamEvent::ContentBlockStart { .. }
    ));
    assert!(matches!(
        events[6].as_ref().unwrap(),
        RawMessageStreamEvent::MessageStop
    ));
}

#[tokio::test]
async fn test_message_stream_accumulation() {
    let data = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg-123","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" World"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let event_stream = parse_sse_stream(byte_stream);
    let stream = MessageStream::new(event_stream);

    let message = stream.get_final_message().await.unwrap();

    assert_eq!(message.id, "msg-123");
    assert_eq!(message.text(), "Hello World");
    assert_eq!(message.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(message.usage.output_tokens, 5);
}

#[tokio::test]
async fn test_text_stream() {
    let data = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg-123","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" World"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_stop
data: {"type":"message_stop"}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let event_stream = parse_sse_stream(byte_stream);
    let stream = MessageStream::new(event_stream);

    let texts: Vec<String> = stream
        .text_stream()
        .filter_map(|r| async { r.ok() })
        .collect()
        .await;

    assert_eq!(texts, vec!["Hello", " World"]);
}

#[tokio::test]
async fn test_parse_ping_event() {
    let data = r#"event: ping
data: {"type":"ping"}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut event_stream = parse_sse_stream(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    assert!(matches!(event, RawMessageStreamEvent::Ping));
}

#[tokio::test]
async fn test_parse_error_event() {
    let data = r#"event: error
data: {"type":"error","error":{"type":"overloaded_error","message":"Server is overloaded"}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut event_stream = parse_sse_stream(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    match event {
        RawMessageStreamEvent::Error { error } => {
            assert_eq!(error.error_type, "overloaded_error");
            assert_eq!(error.message, "Server is overloaded");
        }
        _ => panic!("Expected Error event"),
    }
}

#[tokio::test]
async fn test_chunked_sse_data() {
    // Test that SSE parsing works when data is split across multiple chunks
    let chunk1 = Bytes::from("event: message_start\ndata: {\"type\":");
    let chunk2 = Bytes::from("\"message_start\",\"message\":{\"id\":\"msg-123\",");
    let chunk3 = Bytes::from("\"type\":\"message\",\"role\":\"assistant\",");
    let chunk4 = Bytes::from("\"model\":\"claude-3-5-sonnet\",\"content\":[],");
    let chunk5 = Bytes::from(
        "\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
    );

    let byte_stream = stream::iter(vec![
        Ok(chunk1),
        Ok(chunk2),
        Ok(chunk3),
        Ok(chunk4),
        Ok(chunk5),
    ]);
    let mut event_stream = parse_sse_stream(byte_stream);

    let event = event_stream.next().await.unwrap().unwrap();
    assert!(matches!(event, RawMessageStreamEvent::MessageStart { .. }));
}

#[tokio::test]
async fn test_multi_line_data() {
    // Test multi-line data concatenation (per SSE spec, multiple data lines are joined with \n)
    let data = r#"event: content_block_delta
data: {"type":"content_block_delta",
data: "index":0,
data: "delta":{"type":"text_delta","text":"Hello"}}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let mut event_stream = parse_sse_stream(byte_stream);

    // This tests that multi-line data is properly joined
    let event = event_stream.next().await.unwrap().unwrap();
    match event {
        RawMessageStreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(index, 0);
            assert!(matches!(delta, ContentBlockDelta::TextDelta { text } if text == "Hello"));
        }
        _ => panic!("Expected ContentBlockDelta"),
    }
}

#[tokio::test]
async fn test_tool_use_accumulation() {
    let data = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg-456","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":50,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_123","name":"get_weather","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"location\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"San Francisco\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":25}}

event: message_stop
data: {"type":"message_stop"}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let event_stream = parse_sse_stream(byte_stream);
    let stream = MessageStream::new(event_stream);

    let message = stream.get_final_message().await.unwrap();

    assert_eq!(message.id, "msg-456");
    assert!(message.has_tool_use());

    let tool_uses = message.tool_uses();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].0, "call_123");
    assert_eq!(tool_uses[0].1, "get_weather");
    assert_eq!(
        tool_uses[0].2,
        &serde_json::json!({"location": "San Francisco"})
    );
}
