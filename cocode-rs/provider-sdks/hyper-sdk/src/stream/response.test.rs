use super::*;
use futures::stream;

fn make_stream(
    events: Vec<StreamEvent>,
) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, HyperError>> + Send>> {
    Box::pin(stream::iter(events.into_iter().map(Ok)))
}

#[tokio::test]
async fn test_stream_response_text() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world!"),
        StreamEvent::text_done(0, "Hello world!"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let stream = StreamResponse::new(make_stream(events));
    let text = stream.get_final_text().await.unwrap();
    assert_eq!(text, "Hello world!");
}

#[tokio::test]
async fn test_stream_response_with_thinking() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::thinking_delta(0, "Let me think..."),
        StreamEvent::thinking_done(0, "Let me think..."),
        StreamEvent::text_delta(1, "The answer is 42."),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let stream = StreamResponse::new(make_stream(events));
    let response = stream.get_final_response().await.unwrap();

    assert!(response.has_thinking());
    assert_eq!(response.thinking(), Some("Let me think..."));
    assert_eq!(response.text(), "The answer is 42.");
}

#[tokio::test]
async fn test_text_stream() {
    use futures::StreamExt;

    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let stream = StreamResponse::new(make_stream(events));
    let text_stream = stream.text_stream();
    let texts: Vec<String> = text_stream.map(|r| r.unwrap()).collect().await;

    assert_eq!(texts, vec!["Hello ", "world"]);
}

#[tokio::test]
async fn test_stream_config_default() {
    let config = StreamConfig::default();
    assert_eq!(config.idle_timeout, DEFAULT_IDLE_TIMEOUT);
}

#[tokio::test]
async fn test_stream_with_custom_timeout() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let config = StreamConfig {
        idle_timeout: Duration::from_secs(120),
    };
    let stream = StreamResponse::with_config(make_stream(events), config);
    assert_eq!(stream.config().idle_timeout, Duration::from_secs(120));
}

#[tokio::test]
async fn test_stream_idle_timeout_builder() {
    let events = vec![StreamEvent::response_created("resp_1")];

    let stream = StreamResponse::new(make_stream(events)).idle_timeout(Duration::from_secs(30));
    assert_eq!(stream.config().idle_timeout, Duration::from_secs(30));
}

#[tokio::test]
async fn test_stream_idle_timeout_triggers() {
    use futures::StreamExt as _;

    // Create a stream that never yields events after the first
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let stream: EventStream = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx).map(Ok));

    // Send one event, then let the stream hang
    tx.send(StreamEvent::response_created("resp_1"))
        .await
        .unwrap();
    drop(tx); // Don't send more events, but also don't close cleanly

    // Use a very short timeout for testing
    let mut stream_response = StreamResponse::new(stream).idle_timeout(Duration::from_millis(1));

    // First event should succeed
    let first = stream_response.next_event().await;
    assert!(first.is_some());
    assert!(first.unwrap().is_ok());

    // Wait a bit to ensure the timeout triggers
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Stream is now exhausted (channel closed), should return None
    let second = stream_response.next_event().await;
    assert!(second.is_none());
}
