use super::*;
use crate::stream::parse_sse_stream;
use bytes::Bytes;
use futures::stream;

fn make_response(text: &str) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![Part::text(text)]),
            }),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

fn make_content_stream(responses: Vec<GenerateContentResponse>) -> ContentStream {
    Box::pin(stream::iter(responses.into_iter().map(Ok)))
}

#[tokio::test]
async fn test_stream_next() {
    let responses = vec![make_response("Hello"), make_response(" World")];
    let mut stream = GenerateContentStream::new(make_content_stream(responses));

    let chunk1 = stream.next().await.unwrap().unwrap();
    assert_eq!(chunk1.text(), Some("Hello".to_string()));

    let chunk2 = stream.next().await.unwrap().unwrap();
    assert_eq!(chunk2.text(), Some(" World".to_string()));

    assert!(stream.next().await.is_none());
    assert!(stream.is_closed());
}

#[tokio::test]
async fn test_stream_close() {
    let responses = vec![make_response("Hello"), make_response(" World")];
    let mut stream = GenerateContentStream::new(make_content_stream(responses));

    // Read one chunk
    let _ = stream.next().await;

    // Close early
    stream.close();
    assert!(stream.is_closed());

    // No more chunks
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn test_get_final_text() {
    let responses = vec![make_response("Hello"), make_response(" World")];
    let stream = GenerateContentStream::new(make_content_stream(responses));

    let final_text = stream.get_final_text().await.unwrap();
    assert_eq!(final_text, "Hello World");
}

#[tokio::test]
async fn test_get_final_response() {
    let responses = vec![
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![Part::text("Hello")]),
                }),
                ..Default::default()
            }]),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
                ..Default::default()
            }),
            ..Default::default()
        },
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some("model".to_string()),
                    parts: Some(vec![Part::text(" World")]),
                }),
                finish_reason: Some(FinishReason::Stop),
                ..Default::default()
            }]),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];
    let stream = GenerateContentStream::new(make_content_stream(responses));

    let final_response = stream.get_final_response().await.unwrap();
    assert_eq!(final_response.text(), Some("Hello World".to_string()));

    // Should have latest usage metadata
    let usage = final_response.usage_metadata.unwrap();
    assert_eq!(usage.candidates_token_count, Some(10));
}

#[tokio::test]
async fn test_text_stream() {
    let responses = vec![make_response("A"), make_response("B"), make_response("C")];
    let stream = GenerateContentStream::new(make_content_stream(responses));

    let texts: Vec<String> = stream
        .text_stream()
        .filter_map(|r| async { r.ok() })
        .collect()
        .await;

    assert_eq!(texts, vec!["A", "B", "C"]);
}

#[tokio::test]
async fn test_current_snapshot() {
    let responses = vec![make_response("Hello"), make_response(" World")];
    let mut stream = GenerateContentStream::new(make_content_stream(responses));

    // Before any data
    assert!(stream.current_snapshot().is_none());

    // After first chunk
    let _ = stream.next().await;
    let snapshot = stream.current_snapshot().unwrap();
    assert_eq!(snapshot.text(), Some("Hello".to_string()));

    // After second chunk
    let _ = stream.next().await;
    let snapshot = stream.current_snapshot().unwrap();
    assert_eq!(snapshot.text(), Some("Hello World".to_string()));
}

#[tokio::test]
async fn test_current_text() {
    let responses = vec![make_response("Hello"), make_response(" World")];
    let mut stream = GenerateContentStream::new(make_content_stream(responses));

    assert_eq!(stream.current_text(), "");

    let _ = stream.next().await;
    assert_eq!(stream.current_text(), "Hello");

    let _ = stream.next().await;
    assert_eq!(stream.current_text(), "Hello World");
}

#[tokio::test]
async fn test_into_stream() {
    let responses = vec![make_response("A"), make_response("B")];
    let stream = GenerateContentStream::new(make_content_stream(responses));

    let collected: Vec<_> = stream
        .into_stream()
        .filter_map(|r| async { r.ok() })
        .map(|r| r.text().unwrap_or_default())
        .collect()
        .await;

    assert_eq!(collected, vec!["A", "B"]);
}

#[tokio::test]
async fn test_thought_parts_excluded_from_text() {
    let responses = vec![GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![
                    Part {
                        text: Some("Thinking...".to_string()),
                        thought: Some(true),
                        ..Default::default()
                    },
                    Part {
                        text: Some("Final answer".to_string()),
                        ..Default::default()
                    },
                ]),
            }),
            ..Default::default()
        }]),
        ..Default::default()
    }];
    let stream = GenerateContentStream::new(make_content_stream(responses));

    let final_text = stream.get_final_text().await.unwrap();
    // Thought parts should be excluded
    assert_eq!(final_text, "Final answer");
}

#[tokio::test]
async fn test_empty_stream_error() {
    let stream = GenerateContentStream::new(make_content_stream(vec![]));
    let result = stream.get_final_response().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_with_sse_stream() {
    let data = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]}}]}

data: {"candidates":[{"content":{"role":"model","parts":[{"text":" World"}]}}]}

"#;
    let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
    let content_stream = parse_sse_stream(byte_stream);
    let stream = GenerateContentStream::new(content_stream);

    let final_text = stream.get_final_text().await.unwrap();
    assert_eq!(final_text, "Hello World");
}
