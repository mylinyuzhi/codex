use super::*;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultPart;

#[test]
fn converts_system_as_developer() {
    let prompt = vec![LanguageModelV4Message::System {
        content: "Be helpful".into(),
        provider_options: None,
    }];
    let (items, warnings) =
        convert_to_openai_responses_input(&prompt, SystemMessageMode::Developer);
    assert!(warnings.is_empty());
    assert_eq!(items[0]["role"], "developer");
    assert_eq!(items[0]["content"], "Be helpful");
}

#[test]
fn converts_user_text() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::Text(TextPart {
            text: "Hello".into(),
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (items, _) = convert_to_openai_responses_input(&prompt, SystemMessageMode::System);
    assert_eq!(items[0]["role"], "user");
    assert_eq!(items[0]["content"][0]["type"], "input_text");
    assert_eq!(items[0]["content"][0]["text"], "Hello");
}

#[test]
fn converts_assistant_with_tool_call() {
    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![
            AssistantContentPart::Text(TextPart {
                text: "Let me check".into(),
                provider_metadata: None,
            }),
            AssistantContentPart::ToolCall(ToolCallPart {
                tool_call_id: "call_1".into(),
                tool_name: "get_weather".into(),
                input: serde_json::json!({"city": "SF"}),
                provider_executed: None,
                provider_metadata: None,
            }),
        ],
        provider_options: None,
    }];
    let (items, _) = convert_to_openai_responses_input(&prompt, SystemMessageMode::System);
    // First item: assistant text message
    assert_eq!(items[0]["role"], "assistant");
    // Second item: function_call
    assert_eq!(items[1]["type"], "function_call");
    assert_eq!(items[1]["name"], "get_weather");
}

#[test]
fn converts_tool_result() {
    let prompt = vec![LanguageModelV4Message::Tool {
        content: vec![ToolContentPart::ToolResult(ToolResultPart {
            tool_call_id: "call_1".into(),
            tool_name: "get_weather".into(),
            output: ToolResultContent::Text {
                value: "72F".into(),
                provider_options: None,
            },
            is_error: false,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (items, _) = convert_to_openai_responses_input(&prompt, SystemMessageMode::System);
    assert_eq!(items[0]["type"], "function_call_output");
    assert_eq!(items[0]["call_id"], "call_1");
    assert_eq!(items[0]["output"], "72F");
}
