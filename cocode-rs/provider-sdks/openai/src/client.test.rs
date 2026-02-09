use super::*;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use crate::types::EmbeddingCreateParams;
use crate::types::InputMessage;
use crate::types::ResponseCreateParams;
use crate::types::ResponseStatus;
use crate::types::Tool;

#[test]
fn test_client_requires_api_key() {
    let result = Client::new(ClientConfig::default());
    assert!(matches!(result, Err(OpenAIError::Configuration(_))));
}

#[test]
fn test_client_with_api_key() {
    let result = Client::with_api_key("test-key");
    assert!(result.is_ok());
}

#[test]
fn test_parse_api_error_structured() {
    let body = r#"{"error":{"code":"invalid_request_error","message":"Invalid model"}}"#;
    let error = parse_api_error(400, body, None);
    assert!(matches!(error, OpenAIError::BadRequest(_)));
}

#[test]
fn test_parse_api_error_rate_limit() {
    let body = r#"{"error":{"code":"rate_limit_error","message":"Rate limited"}}"#;
    let error = parse_api_error(429, body, None);
    assert!(matches!(error, OpenAIError::RateLimited { .. }));
}

#[test]
fn test_parse_api_error_context_exceeded() {
    let body = r#"{"error":{"code":"context_length_exceeded","message":"Context too long"}}"#;
    let error = parse_api_error(400, body, None);
    assert!(matches!(error, OpenAIError::ContextWindowExceeded));
}

#[test]
fn test_parse_api_error_quota_exceeded() {
    let body = r#"{"error":{"code":"insufficient_quota","message":"You exceeded your quota"}}"#;
    let error = parse_api_error(429, body, None);
    assert!(matches!(error, OpenAIError::QuotaExceeded));
}

fn make_client(base_url: &str) -> Client {
    let config = ClientConfig::new("test-api-key").base_url(base_url);
    Client::new(config).expect("client creation should succeed")
}

#[tokio::test]
async fn test_responses_create_success() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "id": "resp-123",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Hello! How can I help?"
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 8,
            "total_tokens": 18
        },
        "model": "gpt-4o"
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(header("authorization", "Bearer test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::new("gpt-4o", vec![InputMessage::user_text("Hello!")]);

    let response = client.responses().create(params).await.unwrap();

    assert_eq!(response.id, "resp-123");
    assert_eq!(response.status, ResponseStatus::Completed);
    assert_eq!(response.text(), "Hello! How can I help?");
    assert_eq!(response.usage.total_tokens, 18);
}

#[tokio::test]
async fn test_responses_create_with_tools() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "id": "resp-tool-123",
        "status": "completed",
        "output": [
            {
                "type": "function_call",
                "id": "fc-1",
                "call_id": "call-abc",
                "name": "get_weather",
                "arguments": "{\"city\":\"London\"}"
            }
        ],
        "usage": {
            "input_tokens": 50,
            "output_tokens": 20,
            "total_tokens": 70
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let tool = Tool::function(
        "get_weather",
        Some("Get the weather".to_string()),
        serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )
    .unwrap();

    let params = ResponseCreateParams::new("gpt-4o", vec![InputMessage::user_text("Weather?")])
        .tools(vec![tool]);

    let response = client.responses().create(params).await.unwrap();

    assert!(response.has_function_calls());
    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "get_weather");
}

#[tokio::test]
async fn test_responses_retrieve() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "id": "resp-retrieve-123",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Retrieved response"
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5,
            "total_tokens": 15
        }
    });

    Mock::given(method("GET"))
        .and(path("/responses/resp-retrieve-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let response = client
        .responses()
        .retrieve("resp-retrieve-123")
        .await
        .unwrap();

    assert_eq!(response.id, "resp-retrieve-123");
    assert_eq!(response.text(), "Retrieved response");
}

#[tokio::test]
async fn test_responses_cancel() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "id": "resp-cancel-123",
        "status": "cancelled",
        "output": [],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 0,
            "total_tokens": 10
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses/resp-cancel-123/cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let response = client.responses().cancel("resp-cancel-123").await.unwrap();

    assert_eq!(response.id, "resp-cancel-123");
    assert_eq!(response.status, ResponseStatus::Cancelled);
}

#[tokio::test]
async fn test_embeddings_create() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "object": "list",
        "model": "text-embedding-3-small",
        "data": [
            {
                "object": "embedding",
                "index": 0,
                "embedding": [0.1, 0.2, 0.3, 0.4, 0.5]
            }
        ],
        "usage": {
            "prompt_tokens": 5,
            "total_tokens": 5
        }
    });

    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = EmbeddingCreateParams::new("text-embedding-3-small", "Hello, world!");

    let response = client.embeddings().create(params).await.unwrap();

    assert_eq!(response.model, "text-embedding-3-small");
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.embedding().unwrap().len(), 5);
    assert_eq!(response.dimensions(), Some(5));
}

#[tokio::test]
async fn test_embeddings_multiple_inputs() {
    let mock_server = MockServer::start().await;

    let response_json = serde_json::json!({
        "object": "list",
        "model": "text-embedding-3-small",
        "data": [
            {
                "object": "embedding",
                "index": 0,
                "embedding": [0.1, 0.2, 0.3]
            },
            {
                "object": "embedding",
                "index": 1,
                "embedding": [0.4, 0.5, 0.6]
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "total_tokens": 10
        }
    });

    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = EmbeddingCreateParams::new(
        "text-embedding-3-small",
        vec!["Hello".to_string(), "World".to_string()],
    );

    let response = client.embeddings().create(params).await.unwrap();

    assert_eq!(response.data.len(), 2);
    assert_eq!(response.embeddings().len(), 2);
}

#[tokio::test]
async fn test_rate_limit_error() {
    let mock_server = MockServer::start().await;

    let error_json = serde_json::json!({
        "error": {
            "code": "rate_limit_error",
            "message": "Rate limit exceeded"
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(429).set_body_json(&error_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello");

    let result = client.responses().create(params).await;

    assert!(matches!(result, Err(OpenAIError::RateLimited { .. })));
}

#[tokio::test]
async fn test_context_window_exceeded_error() {
    let mock_server = MockServer::start().await;

    let error_json = serde_json::json!({
        "error": {
            "code": "context_length_exceeded",
            "message": "Context length exceeded"
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(&error_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello");

    let result = client.responses().create(params).await;

    assert!(matches!(result, Err(OpenAIError::ContextWindowExceeded)));
}

#[tokio::test]
async fn test_authentication_error() {
    let mock_server = MockServer::start().await;

    let error_json = serde_json::json!({
        "error": {
            "code": "invalid_api_key",
            "message": "Invalid API key"
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(401).set_body_json(&error_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello");

    let result = client.responses().create(params).await;

    assert!(matches!(result, Err(OpenAIError::Authentication(_))));
}

#[tokio::test]
async fn test_internal_server_error() {
    let mock_server = MockServer::start().await;

    let error_json = serde_json::json!({
        "error": {
            "code": "server_error",
            "message": "Internal server error"
        }
    });

    // Server returns 500 on all 3 attempts (max_retries = 2 means 3 total attempts)
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(500).set_body_json(&error_json))
        .expect(3) // Expects 3 calls due to retry
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello");

    let result = client.responses().create(params).await;

    assert!(matches!(result, Err(OpenAIError::InternalServerError)));
}

#[tokio::test]
async fn test_quota_exceeded_error() {
    let mock_server = MockServer::start().await;

    let error_json = serde_json::json!({
        "error": {
            "code": "insufficient_quota",
            "message": "You exceeded your quota"
        }
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(429).set_body_json(&error_json))
        .mount(&mock_server)
        .await;

    let client = make_client(&mock_server.uri());
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello");

    let result = client.responses().create(params).await;

    assert!(matches!(result, Err(OpenAIError::QuotaExceeded)));
}
