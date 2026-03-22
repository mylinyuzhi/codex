use super::*;
use crate::ClientConfig;
use crate::types::Candidate;
use crate::types::FunctionCall;

fn create_test_client() -> Client {
    // Use a fake API key for testing - requests will fail but structure tests work
    Client::new(ClientConfig::with_api_key("test-api-key").base_url("https://test.example.com"))
        .expect("Failed to create test client")
}

#[test]
fn test_chat_history_management() {
    let client = create_test_client();
    let mut chat = Chat::new(client, "gemini-2.0-flash");

    assert!(chat.history().is_empty());
    assert!(chat.get_history(true).is_empty());
    assert!(chat.get_history(false).is_empty());

    chat.add_to_history(Content::user("Hello"));
    assert_eq!(chat.history().len(), 1);
    assert_eq!(chat.get_history(true).len(), 1);
    assert_eq!(chat.get_history(false).len(), 1);

    chat.add_to_history(Content::model("Hi there!"));
    assert_eq!(chat.history().len(), 2);

    chat.clear_history();
    assert!(chat.history().is_empty());
    assert!(chat.get_history(false).is_empty());
}

#[test]
fn test_chat_builder() {
    let client = create_test_client();

    let chat = ChatBuilder::new(client, "gemini-2.0-flash")
        .system_instruction("You are a helpful assistant")
        .temperature(0.7)
        .max_output_tokens(1024)
        .build();

    assert!(chat.config.is_some());
    let config = chat.config.as_ref().unwrap();
    assert_eq!(config.max_output_tokens, Some(1024));
    assert!(config.system_instruction.is_some());
    // Check temperature with approximate comparison (f32 precision)
    assert!((config.temperature.unwrap() - 0.7).abs() < 0.001);
}

#[test]
fn test_is_valid_response_with_text() {
    let response = GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![Part::text("Hello!")]),
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    assert!(is_valid_response(&response));
}

#[test]
fn test_is_valid_response_with_function_call() {
    let response = GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![Part {
                    function_call: Some(FunctionCall::new(
                        "get_weather",
                        serde_json::json!({"city": "Tokyo"}),
                    )),
                    ..Default::default()
                }]),
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    assert!(is_valid_response(&response));
}

#[test]
fn test_is_valid_response_empty_parts() {
    let response = GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(Vec::new()), // Empty parts = invalid
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    assert!(!is_valid_response(&response));
}

#[test]
fn test_is_valid_response_no_candidates() {
    let response = GenerateContentResponse {
        candidates: None,
        ..Default::default()
    };
    assert!(!is_valid_response(&response));
}

#[test]
fn test_is_valid_response_empty_part() {
    let response = GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![Part::default()]), // Empty Part = invalid
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    assert!(!is_valid_response(&response));
}

#[test]
fn test_chat_with_initial_history() {
    let client = create_test_client();
    let initial_history = vec![Content::user("Hello"), Content::model("Hi there!")];

    let chat = Chat::with_history(client, "gemini-2.0-flash", initial_history);

    assert_eq!(chat.history().len(), 2);
    assert_eq!(chat.get_history(true).len(), 2);
    assert_eq!(chat.get_history(false).len(), 2);
}

// ========== Python SDK Alignment Tests ==========

#[test]
fn test_history_alternates_user_model() {
    // Test that history maintains user -> model alternation
    let client = create_test_client();
    let mut chat = Chat::new(client, "gemini-2.0-flash");

    // Manually add history to test structure
    chat.add_to_history(Content::user("Question 1"));
    chat.add_to_history(Content::model("Answer 1"));
    chat.add_to_history(Content::user("Question 2"));
    chat.add_to_history(Content::model("Answer 2"));

    let history = chat.history();
    assert_eq!(history.len(), 4);

    // Verify alternating roles
    assert_eq!(history[0].role, Some("user".to_string()));
    assert_eq!(history[1].role, Some("model".to_string()));
    assert_eq!(history[2].role, Some("user".to_string()));
    assert_eq!(history[3].role, Some("model".to_string()));
}

#[test]
fn test_function_call_in_history() {
    // Test that function calls can be properly added to history
    let client = create_test_client();
    let mut chat = Chat::new(client, "gemini-2.0-flash");

    // User message
    chat.add_to_history(Content::user("What's the weather?"));

    // Model response with function call
    let model_response = Content {
        role: Some("model".to_string()),
        parts: Some(vec![Part {
            function_call: Some(FunctionCall {
                id: Some("call_1".to_string()),
                name: Some("get_weather".to_string()),
                args: Some(serde_json::json!({"city": "Tokyo"})),
                partial_args: None,
                will_continue: None,
            }),
            ..Default::default()
        }]),
    };
    chat.add_to_history(model_response);

    // Function response (as user)
    let fn_response = Content {
        role: Some("user".to_string()),
        parts: Some(vec![Part {
            function_response: Some(FunctionResponse {
                id: Some("call_1".to_string()),
                name: Some("get_weather".to_string()),
                response: Some(serde_json::json!({"temp": 20, "condition": "sunny"})),
                will_continue: None,
                scheduling: None,
                parts: None,
            }),
            ..Default::default()
        }]),
    };
    chat.add_to_history(fn_response);

    // Model's final response
    chat.add_to_history(Content::model("It's 20°C and sunny in Tokyo."));

    let history = chat.history();
    assert_eq!(history.len(), 4);

    // Verify function call is in history
    let model_content = &history[1];
    assert!(
        model_content.parts.as_ref().unwrap()[0]
            .function_call
            .is_some()
    );

    // Verify function response is in history
    let fn_resp_content = &history[2];
    assert!(
        fn_resp_content.parts.as_ref().unwrap()[0]
            .function_response
            .is_some()
    );
}

#[test]
fn test_dual_history_consistency() {
    // Test that curated and comprehensive history maintain consistency
    let client = create_test_client();
    let chat = Chat::new(client, "gemini-2.0-flash");

    // Initially both should be empty
    assert!(chat.get_history(true).is_empty()); // curated
    assert!(chat.get_history(false).is_empty()); // comprehensive

    // After adding valid content, both should match
    let mut chat = Chat::new(create_test_client(), "gemini-2.0-flash");
    chat.add_to_history(Content::user("Hello"));
    chat.add_to_history(Content::model("Hi!"));

    let curated = chat.get_history(true);
    let comprehensive = chat.get_history(false);

    // For valid responses, both histories should be equal
    assert_eq!(curated.len(), comprehensive.len());
    assert_eq!(curated.len(), 2);
}

#[test]
fn test_is_valid_response_with_thought_parts() {
    // Test that thought parts are considered valid
    let response = GenerateContentResponse {
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
    };
    assert!(is_valid_response(&response));
}

#[test]
fn test_is_valid_response_thought_only() {
    // Test that response with only thought parts is valid
    let response = GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                role: Some("model".to_string()),
                parts: Some(vec![Part {
                    text: Some("Reasoning...".to_string()),
                    thought: Some(true),
                    thought_signature: Some(b"sig123".to_vec()),
                    ..Default::default()
                }]),
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    assert!(is_valid_response(&response));
}
