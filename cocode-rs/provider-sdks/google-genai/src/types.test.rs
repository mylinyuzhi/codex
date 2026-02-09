use super::*;

#[test]
fn test_request_serialization_structure() {
    // Test that the request is serialized with correct field names
    let request = GenerateContentRequest {
        contents: vec![Content::user("Hello")],
        system_instruction: Some(Content {
            parts: Some(vec![Part::text("You are helpful")]),
            role: Some("user".to_string()),
        }),
        generation_config: Some(GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            ..Default::default()
        }),
        safety_settings: None,
        tools: Some(vec![Tool::functions(vec![FunctionDeclaration::new(
            "test_func",
        )])]),
        tool_config: None,
    };

    let json = serde_json::to_value(&request).expect("serialization failed");

    // Verify top-level fields
    assert!(json.get("contents").is_some());
    assert!(json.get("systemInstruction").is_some());
    assert!(json.get("generationConfig").is_some());
    assert!(json.get("tools").is_some());

    // Verify generationConfig contains temperature (camelCase)
    let gen_config = json.get("generationConfig").unwrap();
    // Check temperature exists and is approximately 0.7 (f32 precision)
    let temp = gen_config.get("temperature").unwrap().as_f64().unwrap();
    assert!((temp - 0.7).abs() < 0.001);
    assert_eq!(
        gen_config.get("maxOutputTokens"),
        Some(&serde_json::json!(1024))
    );

    // Verify contents structure
    let contents = json.get("contents").unwrap().as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].get("role"), Some(&serde_json::json!("user")));
}

#[test]
fn test_response_deserialization() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"text": "Hello!"}],
                "role": "model"
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 20,
            "totalTokenCount": 30
        }
    }"#;

    let response: GenerateContentResponse =
        serde_json::from_str(json).expect("deserialization failed");

    assert!(response.candidates.is_some());
    assert_eq!(response.text(), Some("Hello!".to_string()));
    assert_eq!(response.finish_reason(), Some(FinishReason::Stop));

    let usage = response.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, Some(10));
    assert_eq!(usage.candidates_token_count, Some(20));
    assert_eq!(usage.total_token_count, Some(30));
}

#[test]
fn test_function_call_response_deserialization() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "Tokyo"}
                    }
                }],
                "role": "model"
            },
            "finishReason": "STOP"
        }]
    }"#;

    let response: GenerateContentResponse =
        serde_json::from_str(json).expect("deserialization failed");

    let calls = response.function_calls().expect("no function calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, Some("get_weather".to_string()));
    assert_eq!(
        calls[0].args,
        Some(serde_json::json!({"location": "Tokyo"}))
    );
}

#[test]
fn test_part_constructors() {
    // Text part
    let part = Part::text("hello");
    assert_eq!(part.text, Some("hello".to_string()));
    assert!(part.inline_data.is_none());

    // Image part from bytes
    let part = Part::from_bytes(&[1, 2, 3], "image/png");
    assert!(part.inline_data.is_some());
    let blob = part.inline_data.unwrap();
    assert_eq!(blob.mime_type, Some("image/png".to_string()));

    // Function call part
    let part = Part::function_call("test", serde_json::json!({"a": 1}));
    assert!(part.function_call.is_some());
    assert_eq!(part.function_call.unwrap().name, Some("test".to_string()));
}

#[test]
fn test_content_constructors() {
    let user = Content::user("Hello");
    assert_eq!(user.role, Some("user".to_string()));

    let model = Content::model("Hi there");
    assert_eq!(model.role, Some("model".to_string()));
}

#[test]
fn test_tool_constructors() {
    let tool = Tool::functions(vec![
        FunctionDeclaration::new("func1").with_description("A function"),
    ]);
    assert!(tool.function_declarations.is_some());
    assert!(tool.google_search.is_none());

    let search_tool = Tool::google_search();
    assert!(search_tool.google_search.is_some());
    assert!(search_tool.function_declarations.is_none());
}

#[test]
fn test_generate_content_config_has_generation_params() {
    let empty = GenerateContentConfig::default();
    assert!(!empty.has_generation_params());

    let with_temp = GenerateContentConfig {
        temperature: Some(0.5),
        ..Default::default()
    };
    assert!(with_temp.has_generation_params());

    let with_system_only = GenerateContentConfig {
        system_instruction: Some(Content::user("system")),
        ..Default::default()
    };
    assert!(!with_system_only.has_generation_params());
}

#[test]
fn test_request_extensions_builder() {
    let ext = RequestExtensions::new()
        .with_header("X-Custom", "value1")
        .with_param("key", "value2")
        .with_body_field("field", serde_json::json!("value3"));

    assert_eq!(
        ext.headers.as_ref().unwrap().get("X-Custom"),
        Some(&"value1".to_string())
    );
    assert_eq!(
        ext.params.as_ref().unwrap().get("key"),
        Some(&"value2".to_string())
    );
    assert_eq!(
        ext.body.as_ref().unwrap().get("field"),
        Some(&serde_json::json!("value3"))
    );
}

#[test]
fn test_request_extensions_with_body() {
    let ext = RequestExtensions::new().with_body(serde_json::json!({"a": 1, "b": 2}));

    assert_eq!(ext.body, Some(serde_json::json!({"a": 1, "b": 2})));
}

#[test]
fn test_request_extensions_merge_headers() {
    let base = RequestExtensions::new()
        .with_header("A", "1")
        .with_header("B", "2");

    let other = RequestExtensions::new()
        .with_header("B", "3") // Override
        .with_header("C", "4");

    let merged = base.merge(&other);

    let headers = merged.headers.unwrap();
    assert_eq!(headers.get("A"), Some(&"1".to_string()));
    assert_eq!(headers.get("B"), Some(&"3".to_string())); // Overridden
    assert_eq!(headers.get("C"), Some(&"4".to_string()));
}

#[test]
fn test_request_extensions_merge_params() {
    let base = RequestExtensions::new().with_param("x", "1");
    let other = RequestExtensions::new()
        .with_param("x", "2") // Override
        .with_param("y", "3");

    let merged = base.merge(&other);

    let params = merged.params.unwrap();
    assert_eq!(params.get("x"), Some(&"2".to_string())); // Overridden
    assert_eq!(params.get("y"), Some(&"3".to_string()));
}

#[test]
fn test_request_extensions_merge_body() {
    let base = RequestExtensions::new()
        .with_body_field("a", serde_json::json!(1))
        .with_body_field("b", serde_json::json!(2));

    let other = RequestExtensions::new()
        .with_body_field("b", serde_json::json!(3)) // Override
        .with_body_field("c", serde_json::json!(4));

    let merged = base.merge(&other);

    let body = merged.body.unwrap();
    assert_eq!(body.get("a"), Some(&serde_json::json!(1)));
    assert_eq!(body.get("b"), Some(&serde_json::json!(3))); // Overridden
    assert_eq!(body.get("c"), Some(&serde_json::json!(4)));
}

#[test]
fn test_request_extensions_is_empty() {
    assert!(RequestExtensions::new().is_empty());

    assert!(!RequestExtensions::new().with_header("X", "Y").is_empty());
    assert!(!RequestExtensions::new().with_param("X", "Y").is_empty());
    assert!(
        !RequestExtensions::new()
            .with_body_field("X", serde_json::json!("Y"))
            .is_empty()
    );
}

#[test]
fn test_request_extensions_merge_with_none() {
    let ext = RequestExtensions::new().with_header("A", "1");

    // Merge with empty
    let merged = ext.merge(&RequestExtensions::new());
    assert_eq!(
        merged.headers.as_ref().unwrap().get("A"),
        Some(&"1".to_string())
    );

    // Empty merge with non-empty
    let merged = RequestExtensions::new().merge(&ext);
    assert_eq!(
        merged.headers.as_ref().unwrap().get("A"),
        Some(&"1".to_string())
    );
}

// ========== Python SDK Alignment Tests ==========

#[test]
fn test_thinking_config_serialization() {
    let config = ThinkingConfig {
        include_thoughts: Some(true),
        thinking_budget: Some(1024),
        thinking_level: Some(ThinkingLevel::High),
    };

    let json = serde_json::to_value(&config).expect("serialization failed");

    // Verify camelCase field names
    assert_eq!(json["includeThoughts"], true);
    assert_eq!(json["thinkingBudget"], 1024);
    assert_eq!(json["thinkingLevel"], "HIGH");
}

#[test]
fn test_thought_signature_base64_roundtrip() {
    // Test binary data with various byte values including 0x00 and 0xFF
    let original_sig = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
    let part = Part::with_thought_signature(original_sig.clone());

    // Serialize to JSON
    let json = serde_json::to_string(&part).expect("serialization failed");

    // Verify base64 encoding is present (0x00, 0x01, 0x02, 0xFF, 0xFE -> "AAEC//4=")
    assert!(
        json.contains("AAEC//4="),
        "Expected base64 encoding in: {}",
        json
    );

    // Deserialize back
    let parsed: Part = serde_json::from_str(&json).expect("deserialization failed");
    assert_eq!(
        parsed.thought_signature,
        Some(original_sig),
        "Round-trip should preserve exact bytes"
    );
    assert_eq!(parsed.thought, Some(true));
}

#[test]
fn test_multiple_function_calls_in_response() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [
                    {"functionCall": {"id": "call_1", "name": "tool_a", "args": {"x": 1}}},
                    {"functionCall": {"id": "call_2", "name": "tool_b", "args": {"y": 2}}}
                ],
                "role": "model"
            },
            "finishReason": "STOP"
        }]
    }"#;

    let response: GenerateContentResponse =
        serde_json::from_str(json).expect("deserialization failed");
    let calls = response.function_calls().expect("no function calls");

    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, Some("call_1".to_string()));
    assert_eq!(calls[0].name, Some("tool_a".to_string()));
    assert_eq!(calls[1].id, Some("call_2".to_string()));
    assert_eq!(calls[1].name, Some("tool_b".to_string()));
}

#[test]
fn test_thinking_config_nested_in_generation_config() {
    let gen_config = GenerationConfig {
        temperature: Some(0.7),
        thinking_config: Some(ThinkingConfig::with_budget(2048)),
        ..Default::default()
    };

    let request = GenerateContentRequest {
        contents: vec![Content::user("test")],
        system_instruction: None,
        generation_config: Some(gen_config),
        safety_settings: None,
        tools: None,
        tool_config: None,
    };

    let json = serde_json::to_value(&request).expect("serialization failed");

    // Verify thinkingConfig is nested inside generationConfig
    assert_eq!(
        json["generationConfig"]["thinkingConfig"]["thinkingBudget"], 2048,
        "thinkingConfig should be nested in generationConfig"
    );
    // Verify thinkingConfig is NOT at top level
    assert!(
        json.get("thinkingConfig").is_none(),
        "thinkingConfig should not be at top level"
    );
}

#[test]
fn test_usage_metadata_full_fields() {
    let json = r#"{
        "candidates": [{
            "content": {"parts": [{"text": "Hi"}], "role": "model"}
        }],
        "usageMetadata": {
            "promptTokenCount": 100,
            "candidatesTokenCount": 50,
            "totalTokenCount": 150,
            "cachedContentTokenCount": 20,
            "thoughtsTokenCount": 30,
            "toolUsePromptTokenCount": 10
        }
    }"#;

    let response: GenerateContentResponse =
        serde_json::from_str(json).expect("deserialization failed");
    let usage = response.usage_metadata.expect("no usage metadata");

    assert_eq!(usage.prompt_token_count, Some(100));
    assert_eq!(usage.candidates_token_count, Some(50));
    assert_eq!(usage.total_token_count, Some(150));
    assert_eq!(usage.cached_content_token_count, Some(20));
    assert_eq!(usage.thoughts_token_count, Some(30));
    assert_eq!(usage.tool_use_prompt_token_count, Some(10));
}

#[test]
fn test_function_call_with_thought_signature() {
    // Test that function call parts can have thought_signature attached
    let part = Part {
        function_call: Some(FunctionCall {
            id: Some("call_1".to_string()),
            name: Some("search".to_string()),
            args: Some(serde_json::json!({"query": "rust"})),
            partial_args: None,
            will_continue: None,
        }),
        thought_signature: Some(b"sig_for_call_1".to_vec()),
        ..Default::default()
    };

    let json = serde_json::to_string(&part).expect("serialization failed");
    let parsed: Part = serde_json::from_str(&json).expect("deserialization failed");

    assert!(parsed.function_call.is_some());
    assert!(parsed.thought_signature.is_some());
    assert_eq!(
        parsed.thought_signature.unwrap(),
        b"sig_for_call_1".to_vec()
    );
}

#[test]
fn test_thought_part_with_text_and_signature() {
    // Test thought part containing both text and signature (common in reasoning)
    let part = Part {
        text: Some("Let me think about this...".to_string()),
        thought: Some(true),
        thought_signature: Some(b"thought_sig_123".to_vec()),
        ..Default::default()
    };

    let json = serde_json::to_string(&part).expect("serialization failed");
    let parsed: Part = serde_json::from_str(&json).expect("deserialization failed");

    assert_eq!(parsed.text, Some("Let me think about this...".to_string()));
    assert_eq!(parsed.thought, Some(true));
    assert!(parsed.is_thought());
    assert!(parsed.thought_signature.is_some());
}

#[test]
fn test_response_thought_extraction() {
    // Test response.thought_text() and response.thought_signatures()
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [
                    {"text": "Thinking step 1...", "thought": true, "thoughtSignature": "c2lnMQ=="},
                    {"text": "Thinking step 2...", "thought": true},
                    {"text": "Final answer here"}
                ],
                "role": "model"
            }
        }]
    }"#;

    let response: GenerateContentResponse =
        serde_json::from_str(json).expect("deserialization failed");

    // text() should exclude thought parts
    let text = response.text().expect("no text");
    assert_eq!(text, "Final answer here");

    // thought_text() should only include thought parts
    let thought = response.thought_text().expect("no thought text");
    assert!(thought.contains("Thinking step 1"));
    assert!(thought.contains("Thinking step 2"));

    // has_thoughts() should return true
    assert!(response.has_thoughts());

    // thought_signatures() should extract the signature
    let sigs = response.thought_signatures();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0], b"sig1"); // "c2lnMQ==" decodes to "sig1"
}
