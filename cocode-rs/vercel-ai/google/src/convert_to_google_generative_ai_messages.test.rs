use super::*;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::ToolResultPart;

#[test]
fn converts_system_message() {
    let messages = vec![LanguageModelV4Message::system("You are helpful.")];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert!(result.system_instruction.is_some());
    let si = result.system_instruction.unwrap();
    assert_eq!(si.parts.len(), 1);
    assert_eq!(si.parts[0].text, "You are helpful.");
    assert!(result.contents.is_empty());
}

#[test]
fn converts_system_as_user_when_unsupported() {
    let messages = vec![LanguageModelV4Message::system("You are helpful.")];
    let options = ConvertOptions {
        supports_system_instruction: false,
    };
    let result = convert_to_google_generative_ai_messages(&messages, &options);
    assert!(result.system_instruction.is_none());
    assert_eq!(result.contents.len(), 1);
    assert_eq!(result.contents[0].role, GoogleContentRole::User);
}

#[test]
fn converts_user_text_message() {
    let messages = vec![LanguageModelV4Message::user_text("Hello")];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert!(result.system_instruction.is_none());
    assert_eq!(result.contents.len(), 1);
    assert_eq!(result.contents[0].role, GoogleContentRole::User);
    match &result.contents[0].parts[0] {
        GoogleGenerativeAIContentPart::Text { text } => assert_eq!(text, "Hello"),
        _ => panic!("Expected text part"),
    }
}

#[test]
fn converts_assistant_text_message() {
    let messages = vec![LanguageModelV4Message::assistant_text("Hi there")];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert_eq!(result.contents.len(), 1);
    assert_eq!(result.contents[0].role, GoogleContentRole::Model);
}

#[test]
fn converts_tool_call_in_assistant_message() {
    let parts = vec![AssistantContentPart::ToolCall(ToolCallPart::new(
        "call_1",
        "get_weather",
        serde_json::json!({"location": "NYC"}),
    ))];
    let messages = vec![LanguageModelV4Message::assistant(parts)];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert_eq!(result.contents.len(), 1);
    match &result.contents[0].parts[0] {
        GoogleGenerativeAIContentPart::FunctionCall { function_call } => {
            assert_eq!(function_call.name, "get_weather");
        }
        _ => panic!("Expected function call part"),
    }
}

#[test]
fn converts_tool_result_message() {
    let parts = vec![ToolContentPart::ToolResult(ToolResultPart::new(
        "call_1",
        "get_weather",
        ToolResultContent::text("Sunny, 72F"),
    ))];
    let messages = vec![LanguageModelV4Message::tool(parts)];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert_eq!(result.contents.len(), 1);
    assert_eq!(result.contents[0].role, GoogleContentRole::User);
    match &result.contents[0].parts[0] {
        GoogleGenerativeAIContentPart::FunctionResponse { function_response } => {
            assert_eq!(function_response.name, "get_weather");
        }
        _ => panic!("Expected function response part"),
    }
}

#[test]
fn converts_file_with_base64() {
    use vercel_ai_provider::DataContent;
    use vercel_ai_provider::FilePart;

    let parts = vec![UserContentPart::File(FilePart::new(
        DataContent::from_base64("aGVsbG8="),
        "image/png",
    ))];
    let messages = vec![LanguageModelV4Message::user(parts)];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    match &result.contents[0].parts[0] {
        GoogleGenerativeAIContentPart::InlineData { inline_data } => {
            assert_eq!(inline_data.mime_type, "image/png");
            assert_eq!(inline_data.data, "aGVsbG8=");
        }
        _ => panic!("Expected inline data part"),
    }
}

#[test]
fn skips_reasoning_parts() {
    let parts = vec![
        AssistantContentPart::reasoning("thinking..."),
        AssistantContentPart::text("answer"),
    ];
    let messages = vec![LanguageModelV4Message::assistant(parts)];
    let result = convert_to_google_generative_ai_messages(&messages, &ConvertOptions::default());
    assert_eq!(result.contents[0].parts.len(), 1);
    match &result.contents[0].parts[0] {
        GoogleGenerativeAIContentPart::Text { text } => assert_eq!(text, "answer"),
        _ => panic!("Expected text part"),
    }
}
