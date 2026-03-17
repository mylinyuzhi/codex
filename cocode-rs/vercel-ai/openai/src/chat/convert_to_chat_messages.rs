use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::DataContent;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::ToolContentPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::Warning;

use crate::openai_capabilities::SystemMessageMode;

/// Convert a `LanguageModelV4Prompt` into OpenAI Chat Completions API messages.
///
/// Returns `(messages, warnings)`.
pub fn convert_to_openai_chat_messages(
    prompt: &LanguageModelV4Prompt,
    system_message_mode: SystemMessageMode,
) -> (Vec<Value>, Vec<Warning>) {
    let mut messages = Vec::new();
    let mut warnings = Vec::new();

    for msg in prompt {
        match msg {
            LanguageModelV4Message::System {
                content,
                provider_options: _,
            } => match system_message_mode {
                SystemMessageMode::System => {
                    messages.push(json!({ "role": "system", "content": content }));
                }
                SystemMessageMode::Developer => {
                    messages.push(json!({ "role": "developer", "content": content }));
                }
                SystemMessageMode::Remove => {
                    warnings.push(Warning::Other {
                        message: "system messages are removed for this model".into(),
                    });
                }
            },

            LanguageModelV4Message::User {
                content,
                provider_options,
            } => {
                let parts = convert_user_parts(content, provider_options);
                // Single text part can be simplified to just a string
                if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
                {
                    messages.push(json!({
                        "role": "user",
                        "content": parts[0]["text"]
                    }));
                } else {
                    messages.push(json!({
                        "role": "user",
                        "content": parts
                    }));
                }
            }

            LanguageModelV4Message::Assistant {
                content,
                provider_options: _,
            } => {
                let (text, tool_calls) = convert_assistant_parts(content);
                let mut msg = json!({ "role": "assistant" });
                if let Some(text) = text {
                    msg["content"] = Value::String(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = Value::Array(tool_calls);
                }
                messages.push(msg);
            }

            LanguageModelV4Message::Tool {
                content,
                provider_options: _,
            } => {
                for part in content {
                    match part {
                        ToolContentPart::ToolResult(result) => {
                            let output = serialize_tool_result_content(&result.output);
                            messages.push(json!({
                                "role": "tool",
                                "tool_call_id": result.tool_call_id,
                                "content": output,
                            }));
                        }
                        ToolContentPart::ToolApprovalResponse(_) => {
                            // Approval responses are not supported in Chat API
                        }
                    }
                }
            }
        }
    }

    (messages, warnings)
}

/// Convert user content parts to OpenAI format.
fn convert_user_parts(
    parts: &[UserContentPart],
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> Vec<Value> {
    let image_detail = provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| v.get("imageDetail"))
        .and_then(|v| v.as_str())
        .map(String::from);

    parts
        .iter()
        .map(|part| match part {
            UserContentPart::Text(text_part) => {
                json!({ "type": "text", "text": text_part.text })
            }
            UserContentPart::File(file_part) => {
                let media_type = &file_part.media_type;
                if media_type.starts_with("image/") {
                    // Convert wildcard image/* to image/jpeg
                    let effective_type = if media_type == "image/*" {
                        "image/jpeg"
                    } else {
                        media_type.as_str()
                    };
                    let url = data_content_to_url(&file_part.data, effective_type);
                    let mut image_url = json!({ "url": url });
                    if let Some(ref detail) = image_detail {
                        image_url["detail"] = Value::String(detail.clone());
                    }
                    json!({ "type": "image_url", "image_url": image_url })
                } else if media_type == "audio/wav"
                    || media_type == "audio/mp3"
                    || media_type == "audio/mpeg"
                {
                    let b64 = data_content_to_base64(&file_part.data);
                    let format = if media_type == "audio/wav" {
                        "wav"
                    } else {
                        "mp3"
                    };
                    json!({
                        "type": "input_audio",
                        "input_audio": { "data": b64, "format": format }
                    })
                } else if media_type == "application/pdf" {
                    // Check if data is a file ID (string starting with "file-")
                    if let DataContent::Base64(ref s) = file_part.data
                        && s.starts_with("file-")
                    {
                        return json!({
                            "type": "file",
                            "file": { "file_id": s }
                        });
                    }
                    let b64 = data_content_to_base64(&file_part.data);
                    json!({
                        "type": "file",
                        "file": {
                            "file_data": format!("data:{media_type};base64,{b64}"),
                        }
                    })
                } else {
                    // Generic file
                    let b64 = data_content_to_base64(&file_part.data);
                    json!({
                        "type": "file",
                        "file": {
                            "file_data": format!("data:{media_type};base64,{b64}"),
                        }
                    })
                }
            }
        })
        .collect()
}

/// Convert assistant content parts to (concatenated text, tool_calls array).
fn convert_assistant_parts(parts: &[AssistantContentPart]) -> (Option<String>, Vec<Value>) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for part in parts {
        match part {
            AssistantContentPart::Text(text_part) => {
                text_parts.push(text_part.text.clone());
            }
            AssistantContentPart::ToolCall(tc) => {
                tool_calls.push(json!({
                    "id": tc.tool_call_id,
                    "type": "function",
                    "function": {
                        "name": tc.tool_name,
                        "arguments": serde_json::to_string(&tc.input).unwrap_or_default(),
                    }
                }));
            }
            // Reasoning, File, Source, ToolResult, ToolApprovalRequest — skip
            _ => {}
        }
    }

    let text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    (text, tool_calls)
}

/// Serialize a tool result content to a string for the Chat API.
fn serialize_tool_result_content(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text { value, .. } => value.clone(),
        ToolResultContent::Json { value, .. } => serde_json::to_string(value).unwrap_or_default(),
        ToolResultContent::ErrorText { value, .. } => value.clone(),
        ToolResultContent::ErrorJson { value, .. } => {
            serde_json::to_string(value).unwrap_or_default()
        }
        ToolResultContent::ExecutionDenied { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "Tool execution denied.".into()),
        ToolResultContent::Content { value, .. } => {
            // Serialize content parts to a string representation
            let parts: Vec<String> = value
                .iter()
                .filter_map(|part| match part {
                    vercel_ai_provider::ToolResultContentPart::Text { text, .. } => {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
    }
}

/// Convert DataContent to a URL string (for images).
fn data_content_to_url(data: &DataContent, media_type: &str) -> String {
    match data {
        DataContent::Url(url) => url.clone(),
        DataContent::Base64(b64) => format!("data:{media_type};base64,{b64}"),
        DataContent::Bytes(bytes) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            format!("data:{media_type};base64,{b64}")
        }
    }
}

/// Convert DataContent to base64 string.
fn data_content_to_base64(data: &DataContent) -> String {
    match data {
        DataContent::Base64(b64) => b64.clone(),
        DataContent::Bytes(bytes) => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        DataContent::Url(url) => {
            // If it's already a data URL, extract the base64
            if let Some(idx) = url.find(";base64,") {
                url[idx + 8..].to_string()
            } else {
                url.clone()
            }
        }
    }
}

#[cfg(test)]
#[path = "convert_to_chat_messages.test.rs"]
mod tests;
