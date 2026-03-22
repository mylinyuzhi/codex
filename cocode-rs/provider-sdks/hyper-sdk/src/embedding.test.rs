use super::*;

#[test]
fn test_embed_request_single() {
    let request = EmbedRequest::single("Hello world");
    assert_eq!(request.input.len(), 1);
    assert_eq!(request.input[0], "Hello world");
}

#[test]
fn test_embed_request_batch() {
    let request =
        EmbedRequest::batch(vec!["Hello".to_string(), "World".to_string()]).dimensions(256);

    assert_eq!(request.input.len(), 2);
    assert_eq!(request.dimensions, Some(256));
}

#[test]
fn test_cosine_similarity() {
    let e1 = Embedding::new(0, vec![1.0, 0.0, 0.0]);
    let e2 = Embedding::new(1, vec![1.0, 0.0, 0.0]);
    let e3 = Embedding::new(2, vec![0.0, 1.0, 0.0]);

    // Same vector = 1.0
    assert!((e1.cosine_similarity(&e2) - 1.0).abs() < 0.001);

    // Orthogonal vectors = 0.0
    assert!((e1.cosine_similarity(&e3) - 0.0).abs() < 0.001);
}

#[test]
fn test_embed_response() {
    let response = EmbedResponse::new(
        "text-embedding-3-small",
        vec![
            Embedding::new(0, vec![0.1, 0.2, 0.3]),
            Embedding::new(1, vec![0.4, 0.5, 0.6]),
        ],
    );

    assert_eq!(response.embeddings.len(), 2);
    assert_eq!(response.first().unwrap().dimensions(), 3);
}
