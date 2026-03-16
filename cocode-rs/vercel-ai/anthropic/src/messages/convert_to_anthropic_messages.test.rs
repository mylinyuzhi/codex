use super::*;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;

#[test]
fn converts_system_message() {
    let prompt = vec![LanguageModelV4Message::System {
        content: "You are a helpful assistant.".into(),
        provider_options: None,
    }];
    let (system, messages, warnings) = convert_to_anthropic_messages(&prompt, true);
    assert!(warnings.is_empty());
    assert!(messages.is_empty());
    let system = system.unwrap_or_else(|| panic!("expected system"));
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["type"], "text");
    assert_eq!(system[0]["text"], "You are a helpful assistant.");
}

#[test]
fn converts_user_text_message() {
    let prompt = vec![LanguageModelV4Message::user_text("Hello")];
    let (system, messages, warnings) = convert_to_anthropic_messages(&prompt, true);
    assert!(system.is_none());
    assert!(warnings.is_empty());
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    let content = messages[0]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("expected array"));
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Hello");
}

#[test]
fn converts_assistant_text_message() {
    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::Text(TextPart {
            text: "Hi there!".into(),
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (_, messages, _) = convert_to_anthropic_messages(&prompt, true);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "assistant");
    let content = messages[0]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("expected array"));
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Hi there!");
}

#[test]
fn converts_assistant_tool_call() {
    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "tc_1".into(),
            tool_name: "get_weather".into(),
            input: serde_json::json!({"city": "SF"}),
            provider_executed: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (_, messages, _) = convert_to_anthropic_messages(&prompt, true);
    let content = messages[0]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("expected array"));
    assert_eq!(content[0]["type"], "tool_use");
    assert_eq!(content[0]["id"], "tc_1");
    assert_eq!(content[0]["name"], "get_weather");
    assert_eq!(content[0]["input"]["city"], "SF");
}

#[test]
fn converts_tool_result() {
    let prompt = vec![LanguageModelV4Message::Tool {
        content: vec![ToolContentPart::ToolResult(
            vercel_ai_provider::content::ToolResultPart {
                tool_call_id: "tc_1".into(),
                tool_name: String::new(),
                is_error: false,
                output: ToolResultContent::Text {
                    value: "Sunny, 72°F".into(),
                    provider_options: None,
                },
                provider_metadata: None,
            },
        )],
        provider_options: None,
    }];
    let (_, messages, _) = convert_to_anthropic_messages(&prompt, true);
    assert_eq!(messages[0]["role"], "user");
    let content = messages[0]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("expected array"));
    assert_eq!(content[0]["type"], "tool_result");
    assert_eq!(content[0]["tool_use_id"], "tc_1");
    assert_eq!(content[0]["content"], "Sunny, 72°F");
}

#[test]
fn system_is_none_when_no_system_messages() {
    let prompt = vec![LanguageModelV4Message::user_text("Hi")];
    let (system, _, _) = convert_to_anthropic_messages(&prompt, true);
    assert!(system.is_none());
}

#[test]
fn multiple_system_messages_concatenated() {
    let prompt = vec![
        LanguageModelV4Message::System {
            content: "First instruction.".into(),
            provider_options: None,
        },
        LanguageModelV4Message::System {
            content: "Second instruction.".into(),
            provider_options: None,
        },
    ];
    let (system, _, _) = convert_to_anthropic_messages(&prompt, true);
    let system = system.unwrap_or_else(|| panic!("expected system"));
    assert_eq!(system.len(), 2);
}
