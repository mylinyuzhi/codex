use super::*;

#[test]
fn test_client_config_default() {
    let config = ClientConfig::default();
    assert!(config.api_key.is_none());
    assert!(config.base_url.is_none());
    assert_eq!(config.timeout_secs, Some(600));
}

#[test]
fn test_client_config_with_api_key() {
    let config = ClientConfig::with_api_key("test-key");
    assert_eq!(config.api_key, Some("test-key".to_string()));
}

#[test]
fn test_model_url() {
    let client = Client {
        http_client: reqwest::Client::new(),
        api_key: "test".to_string(),
        base_url: GEMINI_API_BASE_URL.to_string(),
        api_version: DEFAULT_API_VERSION.to_string(),
        default_extensions: None,
        request_hook: None,
    };

    assert_eq!(
        client.model_url("gemini-2.0-flash", "generateContent"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
    );

    assert_eq!(
        client.model_url("models/gemini-2.0-flash", "generateContent"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
    );
}

#[test]
fn test_model_url_streaming() {
    let client = Client {
        http_client: reqwest::Client::new(),
        api_key: "test".to_string(),
        base_url: GEMINI_API_BASE_URL.to_string(),
        api_version: DEFAULT_API_VERSION.to_string(),
        default_extensions: None,
        request_hook: None,
    };

    // Base URL without ?alt=sse (added by generate_content_stream)
    assert_eq!(
        client.model_url("gemini-2.0-flash", "streamGenerateContent"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent"
    );

    // Full streaming URL (as used in generate_content_stream)
    let streaming_url = format!(
        "{}?alt=sse",
        client.model_url("gemini-2.0-flash", "streamGenerateContent")
    );
    assert_eq!(
        streaming_url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
    );
}

#[test]
fn test_model_url_custom_base() {
    let client = Client {
        http_client: reqwest::Client::new(),
        api_key: "test".to_string(),
        base_url: "https://search.bytedance.net/gpt/openapi/online/multimodal/crawl/google/v1"
            .to_string(),
        api_version: "v1".to_string(),
        default_extensions: None,
        request_hook: None,
    };

    assert_eq!(
        client.model_url("gemini-2.5-flash", "generateContent"),
        "https://search.bytedance.net/gpt/openapi/online/multimodal/crawl/google/v1/models/gemini-2.5-flash:generateContent"
    );

    assert_eq!(
        client.model_url("gemini-2.5-flash", "streamGenerateContent"),
        "https://search.bytedance.net/gpt/openapi/online/multimodal/crawl/google/v1/models/gemini-2.5-flash:streamGenerateContent"
    );
}
