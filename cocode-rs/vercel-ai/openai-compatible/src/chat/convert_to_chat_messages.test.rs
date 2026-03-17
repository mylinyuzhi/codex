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
    let (msgs, warnings) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "You are helpful");
}

#[test]
fn converts_user_text_message() {
    let prompt = vec![user_text("Hello")];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Hello");
}

#[test]
fn converts_assistant_text_message() {
    let prompt = vec![assistant_text("Hi there")];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
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
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    let tc = &msgs[0]["tool_calls"][0];
    assert_eq!(tc["extra_content"]["google"]["thought_signature"], "sig123");
}

// --- Part metadata propagation tests ---

fn make_openai_compatible_metadata(
    fields: serde_json::Map<String, serde_json::Value>,
) -> ProviderMetadata {
    let mut meta = std::collections::HashMap::new();
    meta.insert("openaiCompatible".into(), serde_json::Value::Object(fields));
    ProviderMetadata(meta)
}

#[test]
fn user_single_text_spreads_part_metadata_on_message() {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "cacheControl".into(),
        serde_json::json!({"type": "ephemeral"}),
    );
    let pm = make_openai_compatible_metadata(fields);

    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::Text(TextPart {
            text: "Hello".into(),
            provider_metadata: Some(pm),
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Hello");
    assert_eq!(
        msgs[0]["cacheControl"],
        serde_json::json!({"type": "ephemeral"})
    );
}

#[test]
fn user_multi_part_spreads_part_metadata_on_parts() {
    let mut fields = serde_json::Map::new();
    fields.insert("customField".into(), serde_json::json!("value1"));
    let pm = make_openai_compatible_metadata(fields);

    let prompt = vec![LanguageModelV4Message::User {
        content: vec![
            UserContentPart::Text(TextPart {
                text: "Hi".into(),
                provider_metadata: Some(pm),
            }),
            UserContentPart::Text(TextPart {
                text: "World".into(),
                provider_metadata: None,
            }),
        ],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    let content = &msgs[0]["content"];
    assert_eq!(content[0]["customField"], "value1");
    assert!(content[1].get("customField").is_none());
}

#[test]
fn user_multi_part_spreads_message_metadata_on_message() {
    let mut msg_fields = std::collections::HashMap::new();
    let mut inner = std::collections::HashMap::new();
    inner.insert("msgField".to_string(), serde_json::json!("msgValue"));
    msg_fields.insert("openaiCompatible".to_string(), inner);
    let provider_opts = vercel_ai_provider::ProviderOptions(msg_fields);

    let prompt = vec![LanguageModelV4Message::User {
        content: vec![
            UserContentPart::Text(TextPart {
                text: "A".into(),
                provider_metadata: None,
            }),
            UserContentPart::Text(TextPart {
                text: "B".into(),
                provider_metadata: None,
            }),
        ],
        provider_options: Some(provider_opts),
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    assert_eq!(msgs[0]["msgField"], "msgValue");
}

#[test]
fn tool_result_spreads_part_metadata() {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "cacheControl".into(),
        serde_json::json!({"type": "ephemeral"}),
    );
    let pm = make_openai_compatible_metadata(fields);

    let prompt = vec![LanguageModelV4Message::Tool {
        content: vec![ToolContentPart::ToolResult(ToolResultPart {
            tool_call_id: "call_1".into(),
            tool_name: "tool1".into(),
            output: ToolResultContent::Text {
                value: "result".into(),
                provider_options: None,
            },
            is_error: false,
            provider_metadata: Some(pm),
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    assert_eq!(msgs[0]["role"], "tool");
    assert_eq!(
        msgs[0]["cacheControl"],
        serde_json::json!({"type": "ephemeral"})
    );
}

#[test]
fn tool_call_spreads_part_metadata() {
    let mut oai_fields = serde_json::Map::new();
    oai_fields.insert("customKey".into(), serde_json::json!("customVal"));
    let mut meta = std::collections::HashMap::new();
    meta.insert(
        "openaiCompatible".into(),
        serde_json::Value::Object(oai_fields),
    );
    let pm = ProviderMetadata(meta);

    let prompt = vec![LanguageModelV4Message::Assistant {
        content: vec![AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "call_789".into(),
            tool_name: "search".into(),
            input: serde_json::json!({}),
            provider_executed: None,
            provider_metadata: Some(pm),
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    let tc = &msgs[0]["tool_calls"][0];
    assert_eq!(tc["customKey"], "customVal");
}

#[test]
fn image_part_spreads_metadata() {
    let mut fields = serde_json::Map::new();
    fields.insert("custom".into(), serde_json::json!(true));
    let pm = make_openai_compatible_metadata(fields);

    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "image/png".into(),
            filename: None,
            provider_metadata: Some(pm),
        })],
        provider_options: None,
    }];
    let (msgs, _) = convert_to_openai_compatible_chat_messages(&prompt).unwrap();
    let content = &msgs[0]["content"][0];
    assert_eq!(content["type"], "image_url");
    assert_eq!(content["custom"], true);
}

// --- Error handling tests ---

#[test]
fn pdf_url_returns_error() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Url("https://example.com/doc.pdf".into()),
            media_type: "application/pdf".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let result = convert_to_openai_compatible_chat_messages(&prompt);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("PDF file parts with URLs"));
}

#[test]
fn unsupported_audio_format_returns_error() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "audio/ogg".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let result = convert_to_openai_compatible_chat_messages(&prompt);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("audio media type audio/ogg"));
}

#[test]
fn unsupported_file_media_type_returns_error() {
    let prompt = vec![LanguageModelV4Message::User {
        content: vec![UserContentPart::File(FilePart {
            data: DataContent::Base64("dGVzdA==".into()),
            media_type: "application/octet-stream".into(),
            filename: None,
            provider_metadata: None,
        })],
        provider_options: None,
    }];
    let result = convert_to_openai_compatible_chat_messages(&prompt);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("file part media type application/octet-stream"));
}
