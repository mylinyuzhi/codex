use super::*;

#[test]
fn test_embedding_input_from_str() {
    let input: EmbeddingInput = "hello".into();
    match input {
        EmbeddingInput::Single(s) => assert_eq!(s, "hello"),
        _ => panic!("expected Single"),
    }
}

#[test]
fn test_embedding_input_from_vec() {
    let input: EmbeddingInput = vec!["hello", "world"].into();
    match input {
        EmbeddingInput::Multiple(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], "hello");
            assert_eq!(v[1], "world");
        }
        _ => panic!("expected Multiple"),
    }
}

#[test]
fn test_embedding_create_params_builder() {
    let params = EmbeddingCreateParams::new("text-embedding-3-small", "test text")
        .encoding_format(EncodingFormat::Float)
        .dimensions(256)
        .user("user-123");

    assert_eq!(params.model, "text-embedding-3-small");
    assert_eq!(params.encoding_format, Some(EncodingFormat::Float));
    assert_eq!(params.dimensions, Some(256));
    assert_eq!(params.user, Some("user-123".to_string()));
}

#[test]
fn test_encoding_format_serialization() {
    let float_json = serde_json::to_string(&EncodingFormat::Float).unwrap();
    assert_eq!(float_json, r#""float""#);

    let base64_json = serde_json::to_string(&EncodingFormat::Base64).unwrap();
    assert_eq!(base64_json, r#""base64""#);
}

#[test]
fn test_embedding_response_deserialization() {
    let json = r#"{
        "object": "list",
        "model": "text-embedding-3-small",
        "data": [
            {
                "embedding": [0.1, 0.2, 0.3],
                "index": 0,
                "object": "embedding"
            }
        ],
        "usage": {
            "prompt_tokens": 5,
            "total_tokens": 5
        }
    }"#;

    let response: CreateEmbeddingResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.model, "text-embedding-3-small");
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
    assert_eq!(response.usage.prompt_tokens, 5);
}

#[test]
fn test_embedding_response_helpers() {
    let response = CreateEmbeddingResponse {
        object: "list".to_string(),
        model: "text-embedding-3-small".to_string(),
        data: vec![
            Embedding {
                embedding: vec![0.1, 0.2],
                index: 0,
                object: "embedding".to_string(),
            },
            Embedding {
                embedding: vec![0.3, 0.4],
                index: 1,
                object: "embedding".to_string(),
            },
        ],
        usage: EmbeddingUsage {
            prompt_tokens: 10,
            total_tokens: 10,
        },
    };

    assert_eq!(response.embedding(), Some([0.1, 0.2].as_slice()));
    assert_eq!(response.embeddings().len(), 2);
    assert_eq!(response.dimensions(), Some(2));
}
