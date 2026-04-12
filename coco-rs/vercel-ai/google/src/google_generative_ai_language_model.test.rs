use std::collections::HashMap;
use std::sync::Arc;

use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider_utils::generate_id;

use super::*;

fn make_model() -> GoogleGenerativeAILanguageModel {
    GoogleGenerativeAILanguageModel::new(
        "gemini-2.0-flash",
        GoogleGenerativeAILanguageModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(HashMap::new),
            generate_id: Arc::new(|| generate_id("test")),
            supported_urls: None,
            client: None,
        },
    )
}

#[test]
fn model_id_and_provider() {
    let model = make_model();
    assert_eq!(model.model_id(), "gemini-2.0-flash");
    assert_eq!(model.provider(), "google.generative-ai");
}

#[test]
fn get_args_builds_basic_request() {
    let model = GoogleGenerativeAILanguageModel::new(
        "gemini-2.0-flash",
        GoogleGenerativeAILanguageModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(HashMap::new),
            generate_id: Arc::new(|| "test-id".to_string()),
            supported_urls: None,
            client: None,
        },
    );

    let options = LanguageModelV4CallOptions::new(vec![
        vercel_ai_provider::LanguageModelV4Message::user_text("Hello"),
    ])
    .with_temperature(0.5)
    .with_max_output_tokens(100);

    let (body, _headers, _warnings, _provider_name) = model.get_args(&options).unwrap();

    assert_eq!(body["generationConfig"]["temperature"], 0.5);
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 100);
    assert!(body["contents"].is_array());
}

#[test]
fn response_deserialization() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"text": "Hello world"}]
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 5
        }
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.candidates.len(), 1);
    assert_eq!(
        response.candidates[0].finish_reason.as_deref(),
        Some("STOP")
    );
    assert_eq!(
        response.usage_metadata.as_ref().unwrap().prompt_token_count,
        Some(10)
    );
}

#[test]
fn response_with_function_call() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "NYC"}
                    }
                }]
            },
            "finishReason": "STOP"
        }]
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    let parts = &response.candidates[0].content.as_ref().unwrap().parts;
    assert!(parts[0].function_call.is_some());
    assert_eq!(parts[0].function_call.as_ref().unwrap().name, "get_weather");
}

#[test]
fn response_with_inline_data() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{
                    "inlineData": {
                        "mimeType": "image/png",
                        "data": "aGVsbG8="
                    }
                }]
            },
            "finishReason": "STOP"
        }]
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    let parts = &response.candidates[0].content.as_ref().unwrap().parts;
    assert!(parts[0].inline_data.is_some());
    assert_eq!(
        parts[0].inline_data.as_ref().unwrap().mime_type,
        "image/png"
    );
}

#[test]
fn convert_response_parts_handles_text() {
    let parts = vec![GoogleResponsePart {
        text: Some("Hello".to_string()),
        thought: None,
        thought_signature: None,
        function_call: None,
        inline_data: None,
        executable_code: None,
        code_execution_result: None,
    }];
    let id_gen = || "id".to_string();
    let result = convert_response_parts(&parts, &id_gen);
    assert_eq!(result.len(), 1);
    match &result[0] {
        AssistantContentPart::Text(t) => assert_eq!(t.text, "Hello"),
        _ => panic!("Expected text part"),
    }
}

#[test]
fn convert_response_parts_handles_thought() {
    let parts = vec![GoogleResponsePart {
        text: Some("thinking...".to_string()),
        thought: Some(true),
        thought_signature: None,
        function_call: None,
        inline_data: None,
        executable_code: None,
        code_execution_result: None,
    }];
    let id_gen = || "id".to_string();
    let result = convert_response_parts(&parts, &id_gen);
    assert_eq!(result.len(), 1);
    match &result[0] {
        AssistantContentPart::Reasoning(r) => assert_eq!(r.text, "thinking..."),
        _ => panic!("Expected reasoning part"),
    }
}

#[test]
fn extract_sources_deduplicates() {
    let gm = Some(GroundingMetadata {
        grounding_chunks: Some(vec![
            GroundingChunk {
                web: Some(GroundingWeb {
                    uri: Some("https://example.com".to_string()),
                    title: Some("Example".to_string()),
                }),
                image: None,
                retrieved_context: None,
                maps: None,
            },
            GroundingChunk {
                web: Some(GroundingWeb {
                    uri: Some("https://example.com".to_string()),
                    title: Some("Example Dupe".to_string()),
                }),
                image: None,
                retrieved_context: None,
                maps: None,
            },
        ]),
        web_search_queries: None,
        image_search_queries: None,
        retrieval_queries: None,
        search_entry_point: None,
        grounding_supports: None,
        retrieval_metadata: None,
    });
    let counter = std::sync::atomic::AtomicU32::new(0);
    let id_gen = || {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        format!("src-{n}")
    };
    let sources = extract_sources(&gm, &None, &id_gen);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].url.as_deref(), Some("https://example.com"));
}

#[test]
fn supported_urls_default_empty() {
    let model = make_model();
    let urls = model.supported_urls();
    assert!(urls.is_empty());
}

#[test]
fn supported_urls_with_custom_fn() {
    let model = GoogleGenerativeAILanguageModel::new(
        "gemini-2.0-flash",
        GoogleGenerativeAILanguageModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(HashMap::new),
            generate_id: Arc::new(|| "test-id".to_string()),
            supported_urls: Some(Arc::new(|| {
                let mut m = HashMap::new();
                m.insert(
                    "https".to_string(),
                    vec![Regex::new(r"example\.com").unwrap()],
                );
                m
            })),
            client: None,
        },
    );
    let urls = model.supported_urls();
    assert!(urls.contains_key("https"));
    assert_eq!(urls["https"].len(), 1);
}

#[test]
fn response_with_grounding_metadata() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"text": "Found info"}]
            },
            "finishReason": "STOP",
            "groundingMetadata": {
                "groundingChunks": [{
                    "web": {
                        "uri": "https://example.com/article",
                        "title": "Article Title"
                    }
                }],
                "webSearchQueries": ["test query"]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 3
        }
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    let candidate = &response.candidates[0];
    assert!(candidate.grounding_metadata.is_some());
    let gm = candidate.grounding_metadata.as_ref().unwrap();
    assert_eq!(gm.grounding_chunks.as_ref().unwrap().len(), 1);
    assert_eq!(
        gm.grounding_chunks.as_ref().unwrap()[0]
            .web
            .as_ref()
            .unwrap()
            .uri
            .as_deref(),
        Some("https://example.com/article")
    );
}

#[test]
fn response_with_executable_code() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{
                    "executableCode": {
                        "language": "python",
                        "code": "print('hello')"
                    }
                }]
            },
            "finishReason": "STOP"
        }]
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    let parts = &response.candidates[0].content.as_ref().unwrap().parts;
    assert!(parts[0].executable_code.is_some());
    assert_eq!(
        parts[0].executable_code.as_ref().unwrap().code,
        "print('hello')"
    );
}

#[test]
fn response_with_model_version() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"text": "hi"}]
            },
            "finishReason": "STOP"
        }],
        "modelVersion": "gemini-2.0-flash-001"
    }"#;

    let response: GoogleGenerateContentResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        response.model_version.as_deref(),
        Some("gemini-2.0-flash-001")
    );
}
