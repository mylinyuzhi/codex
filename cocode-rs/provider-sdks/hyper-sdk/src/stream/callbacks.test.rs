use super::*;
use futures::stream;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

fn make_stream(
    events: Vec<StreamEvent>,
) -> Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, HyperError>> + Send>> {
    Box::pin(stream::iter(events.into_iter().map(Ok)))
}

struct CountingCallbacks {
    text_deltas: Arc<AtomicI32>,
    finished: Arc<AtomicI32>,
}

#[async_trait]
impl StreamCallbacks for CountingCallbacks {
    async fn on_text_delta(&mut self, _index: i64, _delta: &str) -> Result<(), HyperError> {
        self.text_deltas.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_finish(&mut self, _reason: FinishReason) -> Result<(), HyperError> {
        self.finished.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn test_callbacks() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world!"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let text_deltas = Arc::new(AtomicI32::new(0));
    let finished = Arc::new(AtomicI32::new(0));

    let callbacks = CountingCallbacks {
        text_deltas: text_deltas.clone(),
        finished: finished.clone(),
    };

    let stream = StreamResponse::new(make_stream(events));
    let _ = stream.process_with_callbacks(callbacks).await.unwrap();

    assert_eq!(text_deltas.load(Ordering::SeqCst), 2);
    assert_eq!(finished.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_collect_text_callbacks() {
    let events = vec![
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world!"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let callbacks = CollectTextCallbacks::new();

    let stream = StreamResponse::new(make_stream(events));
    let _ = stream.process_with_callbacks(callbacks).await;
}
