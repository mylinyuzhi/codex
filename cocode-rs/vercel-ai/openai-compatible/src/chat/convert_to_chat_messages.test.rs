use super::*;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultPart;
use vercel_ai_provider::content::FilePart;

fn system_msg(content: &str) -> LanguageModelV4Message {
    LanguageModelV4Message::System {
        content: content.into(),
        provider_options: None,
    }
}

fn user_text(text: &str) -> LanguageModelV4Message {
    LanguageModelV4Message::User {
        content: vec![UserContentPart::Text(TextPart {
            text: text.into(),
            provider_metadata: None,
        })],
        provider_options: None,
    }
}

fn assistant_text(text: &str) -> LanguageModelV4Message {
    LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::Text(TextPart {
            text: text.into(),
            provider_metadata: None,
        })],
        provider_options: None,
    }
}

#[test]
fn converts_system_message_as_system() {
    let prompt = vec![system_msg("You are helpful")];
    let (msgs, warnings) = convert_to_openai_compatible_chat_messages(&prompt);
    assert!(warnings.is_empty());
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "You are helpful");
}

#[test]
fn converts_user_text_message() {
    let prompt = vec![user_text("Hello")];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Hello");
}

#[test]
fn converts_assistant_text_message() {
    let prompt = vec![assistant_text("Hi there")];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    assert_eq!(msgs[0]["role"], "assistant");
    assert_eq!(msgs[0]["content"], "Hi there");
}

#[test]
fn converts_assistant_tool_call() {
    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "call_123".into(),
            tool_name: "get_weather".into(),
            input: serde_json::json!({"city": "SF"}),
            provider_executed: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    assert_eq!(msgs[0]["role"], "assistant");
    let tc = &msgs[0]["tool_calls"][0];
    assert_eq!(tc["id"], "call_123");
    assert_eq!(tc["type"], "function");
    assert_eq!(tc["function"]["name"], "get_weather");
}

#[test]
fn converts_tool_result() {
    let prompt = vec![LanguageModelV4Message::Tool {
        content: vec![ToolContentPart::ToolResult(ToolResultPart {
            tool_call_id: "call_123".into(),
            tool_name: "get_weather".into(),
            output: ToolResultContent::Text {
                value: "72F and sunny".into(),
                provider_options: None,
            },
            is_error: false,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    assert_eq!(msgs[0]["role"], "tool");
    assert_eq!(msgs[0]["tool_call_id"], "call_123");
    assert_eq!(msgs[0]["content"], "72F and sunny");
}

#[test]
fn includes_reasoning_content_in_assistant_message() {
    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![
            AssistantContentPart::Reasoning(vercel_ai_provider::ReasoningPart::new(
                "Let me think...",
            )),
            AssistantContentPart::Text(TextPart {
                text: "The answer is 42.".into(),
                provider_metadata: None,
            }),
        ],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    assert_eq!(msgs[0]["role"], "assistant");
    assert_eq!(msgs[0]["content"], "The answer is 42.");
    assert_eq!(msgs[0]["reasoning_content"], "Let me think...");
}

#[test]
fn handles_audio_mpeg_as_mp3() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "audio/mpeg".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    let content = &msgs[0]["content"][0];
    assert_eq!(content["type"], "input_audio");
    assert_eq!(content["input_audio"]["format"], "mp3");
}

#[test]
fn handles_image_wildcard_as_jpeg() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "image/*".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    let content = &msgs[0]["content"][0];
    assert_eq!(content["type"], "image_url");
    let url = content["image_url"]["url"].as_str().unwrap();
    assert!(url.starts_with("data:image/jpeg;base64,"));
}

#[test]
fn handles_text_media_type() {
    use base64::Engine;
    let text_data = base64::engine::general_purpose::STANDARD.encode("Hello, world!");
    // Use two parts to avoid single-text-part simplification
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![
            UserContentPart::Text(TextPart {
                text: "Context:".into(),
                provider_metadata: None,
            }),
            UserContentPart::File(FilePart {
                data: DataContent::Base64(text_data),
                media_type: "text/plain".into(),
                filename: None,
                provider_metadata: None,
            }),
        ],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    let content = &msgs[0]["content"];
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Context:");
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[1]["text"], "Hello, world!");
}

#[test]
fn pdf_includes_filename() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "application/pdf".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    let content = &msgs[0]["content"][0];
    assert_eq!(content["type"], "file");
    assert_eq!(content["file"]["filename"], "document.pdf");
}

#[test]
fn tool_call_includes_thought_signature() {
    use std::collections::HashMap;
    use vercel_ai_provider::ProviderMetadata;

    let mut google_meta = serde_json::Map::new();
    google_meta.insert(
        "thoughtSignature".into(),
        serde_json::Value::String("sig123".into()),
    );
    let mut meta = HashMap::new();
    meta.insert("google".into(), serde_json::Value::Object(google_meta));

    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "call_456".into(),
            tool_name: "search".into(),
            input: serde_json::json!({"query": "test"}),
            provider_executed: None,
            provider_metadata: Some(ProviderMetadata(meta)),
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt);
    let tc = &msgs[0]["tool_calls"][0];
    assert_eq!(tc["extra_content"]["google"]["thought_signature"], "sig123");
}
