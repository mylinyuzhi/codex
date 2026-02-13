use super::*;
use crate::types::usage::InputTokensDetails;
use crate::types::usage::OutputTokensDetails;

#[test]
fn test_thinking_config() {
    let enabled = ThinkingConfig::enabled(2048);
    let json = serde_json::to_string(&enabled).unwrap();
    assert!(json.contains(r#""type":"enabled""#));
    assert!(json.contains(r#""budget_tokens":2048"#));

    let disabled = ThinkingConfig::disabled();
    let json = serde_json::to_string(&disabled).unwrap();
    assert!(json.contains(r#""type":"disabled""#));

    let auto = ThinkingConfig::auto();
    let json = serde_json::to_string(&auto).unwrap();
    assert!(json.contains(r#""type":"auto""#));
}

#[test]
fn test_thinking_config_checked() {
    assert!(ThinkingConfig::enabled_checked(1024).is_ok());
    assert!(ThinkingConfig::enabled_checked(2048).is_ok());
    assert!(ThinkingConfig::enabled_checked(1023).is_err());
    assert!(ThinkingConfig::enabled_checked(0).is_err());
}

#[test]
fn test_input_message() {
    let msg = InputMessage::user_text("Hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 1);

    let msg = InputMessage::system("You are helpful");
    assert_eq!(msg.role, Role::System);
}

#[test]
fn test_response_input_text() {
    let input = ResponseInput::from("Hello");
    let json = serde_json::to_string(&input).unwrap();
    assert_eq!(json, r#""Hello""#);
}

#[test]
fn test_response_input_messages() {
    let input = ResponseInput::from(vec![InputMessage::user_text("Hello")]);
    let json = serde_json::to_string(&input).unwrap();
    assert!(json.contains(r#""role":"user""#));
}

#[test]
fn test_response_create_params_builder() {
    let params = ResponseCreateParams::new("gpt-4o", vec![InputMessage::user_text("Hello")])
        .instructions("Be helpful")
        .max_output_tokens(1024)
        .temperature(0.7)
        .thinking(ThinkingConfig::enabled(2048));

    assert_eq!(params.model, "gpt-4o");
    assert_eq!(params.instructions, Some("Be helpful".to_string()));
    assert_eq!(params.max_output_tokens, Some(1024));
    assert_eq!(params.temperature, Some(0.7));
    assert!(params.thinking.is_some());
}

#[test]
fn test_response_create_params_with_text() {
    let params = ResponseCreateParams::with_text("gpt-4o", "Hello world");
    let json = serde_json::to_string(&params).unwrap();
    assert!(json.contains(r#""input":"Hello world""#));
}

#[test]
fn test_prompt_caching_config() {
    let config =
        PromptCachingConfig::with_key("my-key").retention(PromptCacheRetention::TwentyFourHours);

    assert_eq!(config.cache_key, Some("my-key".to_string()));
    assert_eq!(
        config.retention,
        Some(PromptCacheRetention::TwentyFourHours)
    );

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""cache_key":"my-key""#));
    assert!(json.contains(r#""retention":"24h""#));
}

#[test]
fn test_temperature_checked() {
    let params = ResponseCreateParams::new("gpt-4o", vec![]);
    assert!(params.clone().temperature_checked(0.5).is_ok());
    assert!(params.clone().temperature_checked(0.0).is_ok());
    assert!(params.clone().temperature_checked(2.0).is_ok());
    assert!(params.clone().temperature_checked(-0.1).is_err());
    assert!(params.clone().temperature_checked(2.1).is_err());
}

#[test]
fn test_reasoning_config() {
    let config = ReasoningConfig::with_effort(ReasoningEffort::High).with_summary("auto");

    assert_eq!(config.effort, ReasoningEffort::High);
    assert_eq!(config.generate_summary, Some("auto".to_string()));
}

#[test]
fn test_service_tier() {
    let tier = ServiceTier::Priority;
    let json = serde_json::to_string(&tier).unwrap();
    assert_eq!(json, r#""priority""#);

    let tier = ServiceTier::Auto;
    let json = serde_json::to_string(&tier).unwrap();
    assert_eq!(json, r#""auto""#);
}

#[test]
fn test_truncation() {
    let trunc = Truncation::Auto;
    let json = serde_json::to_string(&trunc).unwrap();
    assert_eq!(json, r#""auto""#);

    let trunc = Truncation::Disabled;
    let json = serde_json::to_string(&trunc).unwrap();
    assert_eq!(json, r#""disabled""#);
}

#[test]
fn test_text_config_json_schema() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } }
    });
    let config = TextConfig::json_schema_strict(schema, "person");
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""type":"json_schema""#));
    assert!(json.contains(r#""strict":true"#));
    assert!(json.contains(r#""name":"person""#));
}

#[test]
fn test_text_config_variants() {
    let text = TextConfig::text();
    let json = serde_json::to_string(&text).unwrap();
    assert!(json.contains(r#""type":"text""#));

    let json_obj = TextConfig::json_object();
    let json = serde_json::to_string(&json_obj).unwrap();
    assert!(json.contains(r#""type":"json_object""#));
}

#[test]
fn test_response_create_params_new_fields() {
    let params = ResponseCreateParams::new("gpt-4o", vec![])
        .max_tool_calls(10)
        .parallel_tool_calls(true)
        .service_tier(ServiceTier::Priority)
        .truncation(Truncation::Auto)
        .top_logprobs(5);

    assert_eq!(params.max_tool_calls, Some(10));
    assert_eq!(params.parallel_tool_calls, Some(true));
    assert_eq!(params.service_tier, Some(ServiceTier::Priority));
    assert_eq!(params.truncation, Some(Truncation::Auto));
    assert_eq!(params.top_logprobs, Some(5));
}

#[test]
fn test_top_logprobs_checked() {
    let params = ResponseCreateParams::new("gpt-4o", vec![]);
    assert!(params.clone().top_logprobs_checked(0).is_ok());
    assert!(params.clone().top_logprobs_checked(20).is_ok());
    assert!(params.clone().top_logprobs_checked(10).is_ok());
    assert!(params.clone().top_logprobs_checked(-1).is_err());
    assert!(params.clone().top_logprobs_checked(21).is_err());
}

#[test]
fn test_response_includable() {
    let item = ResponseIncludable::FileSearchCallResults;
    let json = serde_json::to_string(&item).unwrap();
    assert_eq!(json, r#""file_search_call.results""#);

    let item = ResponseIncludable::ComputerCallOutput;
    let json = serde_json::to_string(&item).unwrap();
    assert_eq!(json, r#""computer_call_output""#);

    let item = ResponseIncludable::WebSearchCallResults;
    let json = serde_json::to_string(&item).unwrap();
    assert_eq!(json, r#""web_search_call.results""#);

    let item = ResponseIncludable::CodeInterpreterCallOutputs;
    let json = serde_json::to_string(&item).unwrap();
    assert_eq!(json, r#""code_interpreter_call.outputs""#);
}

#[test]
fn test_incomplete_reason() {
    let reason = IncompleteReason::MaxOutputTokens;
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, r#""max_output_tokens""#);

    let reason = IncompleteReason::ContentFilter;
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, r#""content_filter""#);
}

#[test]
fn test_new_request_params() {
    let params = ResponseCreateParams::new("gpt-4o", vec![])
        .conversation_id("conv-123")
        .background(true)
        .safety_identifier("safety-456")
        .prompt_cache_key("cache-789");

    assert!(matches!(
        params.conversation,
        Some(ConversationParam::Id(_))
    ));
    assert_eq!(params.background, Some(true));
    assert_eq!(params.safety_identifier, Some("safety-456".to_string()));
    assert_eq!(params.prompt_cache_key, Some("cache-789".to_string()));
}

fn make_test_response(output: Vec<OutputItem>) -> Response {
    Response {
        id: "resp-test".to_string(),
        status: ResponseStatus::Completed,
        output,
        usage: Usage::default(),
        created_at: None,
        model: Some("gpt-4o".to_string()),
        object: Some("response".to_string()),
        error: None,
        stop_reason: None,
        completed_at: None,
        incomplete_details: None,
        instructions: None,
        service_tier: None,
        temperature: None,
        parallel_tool_calls: None,
        tools: None,
        tool_choice: None,
        max_output_tokens: None,
        max_tool_calls: None,
        top_p: None,
        reasoning: None,
        text: None,
        truncation: None,
        top_logprobs: None,
        prompt: None,
        prompt_cache_key: None,
        prompt_cache_retention: None,
        safety_identifier: None,
        sdk_http_response: None,
    }
}

#[test]
fn test_response_text_single_message() {
    let response = make_test_response(vec![OutputItem::Message {
        id: Some("msg-1".to_string()),
        role: "assistant".to_string(),
        content: vec![OutputContentBlock::OutputText {
            text: "Hello, world!".to_string(),
            annotations: vec![],
            logprobs: None,
        }],
    }]);
    assert_eq!(response.text(), "Hello, world!");
}

#[test]
fn test_response_text_multiple_messages() {
    let response = make_test_response(vec![
        OutputItem::Message {
            id: Some("msg-1".to_string()),
            role: "assistant".to_string(),
            content: vec![OutputContentBlock::OutputText {
                text: "Hello".to_string(),
                annotations: vec![],
                logprobs: None,
            }],
        },
        OutputItem::Message {
            id: Some("msg-2".to_string()),
            role: "assistant".to_string(),
            content: vec![OutputContentBlock::OutputText {
                text: " world!".to_string(),
                annotations: vec![],
                logprobs: None,
            }],
        },
    ]);
    assert_eq!(response.text(), "Hello world!");
}

#[test]
fn test_response_text_empty() {
    let response = make_test_response(vec![]);
    assert_eq!(response.text(), "");
}

#[test]
fn test_response_function_calls() {
    let response = make_test_response(vec![
        OutputItem::FunctionCall {
            id: Some("fc-1".to_string()),
            call_id: "call-123".to_string(),
            name: "get_weather".to_string(),
            arguments: r#"{"city":"London"}"#.to_string(),
        },
        OutputItem::FunctionCall {
            id: Some("fc-2".to_string()),
            call_id: "call-456".to_string(),
            name: "get_time".to_string(),
            arguments: r#"{"timezone":"UTC"}"#.to_string(),
        },
    ]);
    let calls = response.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0],
        ("call-123", "get_weather", r#"{"city":"London"}"#)
    );
    assert_eq!(calls[1], ("call-456", "get_time", r#"{"timezone":"UTC"}"#));
}

#[test]
fn test_response_has_function_calls_true() {
    let response = make_test_response(vec![OutputItem::FunctionCall {
        id: Some("fc-1".to_string()),
        call_id: "call-123".to_string(),
        name: "test_func".to_string(),
        arguments: "{}".to_string(),
    }]);
    assert!(response.has_function_calls());
}

#[test]
fn test_response_has_function_calls_false() {
    let response = make_test_response(vec![OutputItem::Message {
        id: Some("msg-1".to_string()),
        role: "assistant".to_string(),
        content: vec![],
    }]);
    assert!(!response.has_function_calls());
}

#[test]
fn test_response_has_tool_calls_with_web_search() {
    let response = make_test_response(vec![OutputItem::WebSearchCall {
        id: Some("ws-1".to_string()),
        call_id: "call-ws".to_string(),
        query: Some("test query".to_string()),
        results: None,
        status: Some("completed".to_string()),
    }]);
    assert!(response.has_tool_calls());
}

#[test]
fn test_response_has_tool_calls_with_mcp() {
    let response = make_test_response(vec![OutputItem::McpCall {
        id: Some("mcp-1".to_string()),
        call_id: "call-mcp".to_string(),
        server_label: Some("my-server".to_string()),
        tool_name: Some("my-tool".to_string()),
        arguments: None,
        output: None,
        error: None,
        status: Some("completed".to_string()),
    }]);
    assert!(response.has_tool_calls());
}

#[test]
fn test_response_has_tool_calls_false() {
    let response = make_test_response(vec![OutputItem::Message {
        id: Some("msg-1".to_string()),
        role: "assistant".to_string(),
        content: vec![],
    }]);
    assert!(!response.has_tool_calls());
}

#[test]
fn test_response_reasoning_present() {
    let response = make_test_response(vec![OutputItem::Reasoning {
        id: Some("r-1".to_string()),
        content: "Let me think about this...".to_string(),
        summary: None,
    }]);
    assert_eq!(response.reasoning(), Some("Let me think about this..."));
}

#[test]
fn test_response_reasoning_absent() {
    let response = make_test_response(vec![OutputItem::Message {
        id: Some("msg-1".to_string()),
        role: "assistant".to_string(),
        content: vec![],
    }]);
    assert_eq!(response.reasoning(), None);
}

#[test]
fn test_response_cached_tokens() {
    let mut response = make_test_response(vec![]);
    response.usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        input_tokens_details: InputTokensDetails {
            cached_tokens: 75,
            text_tokens: 25,
            image_tokens: 0,
            audio_tokens: 0,
        },
        output_tokens_details: OutputTokensDetails::default(),
    };
    assert_eq!(response.cached_tokens(), 75);
}

#[test]
fn test_response_web_search_calls() {
    let response = make_test_response(vec![OutputItem::WebSearchCall {
        id: Some("ws-1".to_string()),
        call_id: "call-ws".to_string(),
        query: Some("Rust programming".to_string()),
        results: Some(vec![WebSearchResult {
            title: Some("Rust Lang".to_string()),
            url: Some("https://rust-lang.org".to_string()),
            snippet: Some("A language...".to_string()),
        }]),
        status: Some("completed".to_string()),
    }]);
    let calls = response.web_search_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-ws");
    assert_eq!(calls[0].1, Some("Rust programming"));
    assert!(calls[0].2.is_some());
}

#[test]
fn test_response_file_search_calls() {
    let response = make_test_response(vec![OutputItem::FileSearchCall {
        id: Some("fs-1".to_string()),
        call_id: "call-fs".to_string(),
        queries: vec!["config".to_string(), "settings".to_string()],
        results: Some(vec![FileSearchResult {
            file_id: Some("file-123".to_string()),
            filename: Some("config.json".to_string()),
            score: Some(0.95),
            text: Some("config content".to_string()),
        }]),
        status: Some("completed".to_string()),
    }]);
    let calls = response.file_search_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-fs");
    assert_eq!(calls[0].1, &["config".to_string(), "settings".to_string()]);
}

#[test]
fn test_response_computer_calls() {
    let response = make_test_response(vec![OutputItem::ComputerCall {
        id: Some("cc-1".to_string()),
        call_id: "call-cc".to_string(),
        action: ComputerAction::Click {
            x: 100,
            y: 200,
            button: Some("left".to_string()),
        },
        pending_safety_checks: vec![],
        status: Some("completed".to_string()),
    }]);
    let calls = response.computer_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-cc");
    if let ComputerAction::Click { x, y, .. } = calls[0].1 {
        assert_eq!(*x, 100);
        assert_eq!(*y, 200);
    } else {
        panic!("Expected Click action");
    }
}

#[test]
fn test_response_code_interpreter_calls() {
    let response = make_test_response(vec![OutputItem::CodeInterpreterCall {
        id: Some("ci-1".to_string()),
        call_id: "call-ci".to_string(),
        code: Some("print('Hello')".to_string()),
        outputs: Some(vec![CodeInterpreterOutput::Logs {
            logs: "Hello".to_string(),
        }]),
        status: Some("completed".to_string()),
    }]);
    let calls = response.code_interpreter_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-ci");
    assert_eq!(calls[0].1, Some("print('Hello')"));
}

#[test]
fn test_response_mcp_calls() {
    let response = make_test_response(vec![OutputItem::McpCall {
        id: Some("mcp-1".to_string()),
        call_id: "call-mcp".to_string(),
        server_label: Some("my-server".to_string()),
        tool_name: Some("my-tool".to_string()),
        arguments: Some(serde_json::json!({"key": "value"})),
        output: Some("result".to_string()),
        error: None,
        status: Some("completed".to_string()),
    }]);
    let calls = response.mcp_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call_id, "call-mcp");
    assert_eq!(calls[0].server_label, Some("my-server"));
    assert_eq!(calls[0].tool_name, Some("my-tool"));
    assert_eq!(calls[0].output, Some("result"));
}

#[test]
fn test_response_image_generation_calls() {
    let response = make_test_response(vec![OutputItem::ImageGenerationCall {
        id: Some("ig-1".to_string()),
        call_id: "call-ig".to_string(),
        prompt: Some("A sunset over mountains".to_string()),
        result: Some(ImageGenerationResult {
            url: Some("https://example.com/image.png".to_string()),
            b64_json: None,
            revised_prompt: Some("A beautiful sunset...".to_string()),
        }),
        status: Some("completed".to_string()),
    }]);
    let calls = response.image_generation_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-ig");
    assert_eq!(calls[0].1, Some("A sunset over mountains"));
    assert!(calls[0].2.is_some());
}

#[test]
fn test_response_local_shell_calls() {
    let response = make_test_response(vec![OutputItem::LocalShellCall {
        id: Some("ls-1".to_string()),
        call_id: "call-ls".to_string(),
        command: Some("ls -la".to_string()),
        output: Some("file1.txt\nfile2.txt".to_string()),
        status: Some("completed".to_string()),
    }]);
    let calls = response.local_shell_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-ls");
    assert_eq!(calls[0].1, Some("ls -la"));
    assert_eq!(calls[0].2, Some("file1.txt\nfile2.txt"));
}

#[test]
fn test_deserialize_response_completed_with_message() {
    let json = r#"{
        "id": "resp-abc123",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Hello from the API!"
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5,
            "total_tokens": 15
        },
        "model": "gpt-4o"
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert_eq!(response.id, "resp-abc123");
    assert_eq!(response.status, ResponseStatus::Completed);
    assert_eq!(response.text(), "Hello from the API!");
    assert_eq!(response.usage.input_tokens, 10);
    assert_eq!(response.usage.output_tokens, 5);
}

#[test]
fn test_deserialize_response_with_function_call() {
    let json = r#"{
        "id": "resp-func123",
        "status": "completed",
        "output": [
            {
                "type": "function_call",
                "id": "fc-1",
                "call_id": "call-abc",
                "name": "get_weather",
                "arguments": "{\"city\":\"Tokyo\"}"
            }
        ],
        "usage": {
            "input_tokens": 20,
            "output_tokens": 10,
            "total_tokens": 30
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert!(response.has_function_calls());
    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-abc");
    assert_eq!(calls[0].1, "get_weather");
    assert_eq!(calls[0].2, r#"{"city":"Tokyo"}"#);
}

#[test]
fn test_deserialize_response_with_reasoning() {
    let json = r#"{
        "id": "resp-reason123",
        "status": "completed",
        "output": [
            {
                "type": "reasoning",
                "id": "r-1",
                "content": "Let me analyze this step by step..."
            },
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "The answer is 42."
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 50,
            "output_tokens": 100,
            "total_tokens": 150,
            "output_tokens_details": {
                "reasoning_tokens": 80,
                "text_tokens": 20
            }
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert_eq!(
        response.reasoning(),
        Some("Let me analyze this step by step...")
    );
    assert_eq!(response.text(), "The answer is 42.");
    assert_eq!(response.usage.reasoning_tokens(), 80);
}

#[test]
fn test_deserialize_response_with_web_search() {
    let json = r#"{
        "id": "resp-ws123",
        "status": "completed",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws-1",
                "call_id": "call-ws",
                "query": "Rust programming language",
                "results": [
                    {
                        "title": "Rust Programming Language",
                        "url": "https://rust-lang.org",
                        "snippet": "A language empowering..."
                    }
                ],
                "status": "completed"
            }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5,
            "total_tokens": 15
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert!(response.has_tool_calls());
    let calls = response.web_search_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, Some("Rust programming language"));
}

#[test]
fn test_deserialize_response_failed() {
    let json = r#"{
        "id": "resp-failed123",
        "status": "failed",
        "output": [],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 0,
            "total_tokens": 10
        },
        "error": {
            "code": "content_filter",
            "message": "Content was filtered"
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert_eq!(response.status, ResponseStatus::Failed);
    assert!(response.error.is_some());
    assert_eq!(response.error.as_ref().unwrap().code, "content_filter");
}

#[test]
fn test_deserialize_response_incomplete() {
    let json = r#"{
        "id": "resp-incomplete123",
        "status": "incomplete",
        "output": [
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "This response was truncated..."
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 4096,
            "total_tokens": 4196
        },
        "incomplete_details": {
            "reason": "max_output_tokens"
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert_eq!(response.status, ResponseStatus::Incomplete);
    assert!(response.incomplete_details.is_some());
    assert_eq!(
        response.incomplete_details.as_ref().unwrap().reason,
        Some(IncompleteReason::MaxOutputTokens)
    );
}

#[test]
fn test_deserialize_response_with_cached_tokens() {
    let json = r#"{
        "id": "resp-cached123",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Response with cache hit"
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 1000,
            "output_tokens": 50,
            "total_tokens": 1050,
            "input_tokens_details": {
                "cached_tokens": 950,
                "text_tokens": 50
            }
        }
    }"#;
    let response: Response = serde_json::from_str(json).unwrap();
    assert_eq!(response.cached_tokens(), 950);
    assert_eq!(response.usage.input_text_tokens(), 50);
}
