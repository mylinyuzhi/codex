use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::DataContent;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::ToolContentPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::ToolResultContentPart;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::Warning;

use crate::openai_capabilities::SystemMessageMode;

/// Convert a `LanguageModelV4Prompt` into OpenAI Responses API input items.
///
/// Returns `(input_items, warnings)`.
pub fn convert_to_openai_responses_input(
    prompt: &LanguageModelV4Prompt,
    system_message_mode: SystemMessageMode,
) -> (Vec<Value>, Vec<Warning>) {
    let mut items = Vec::new();
    let mut warnings = Vec::new();

    for msg in prompt {
        match msg {
            LanguageModelV4Message::System {
                content,
                provider_options: _,
            } => match system_message_mode {
                SystemMessageMode::System => {
                    items.push(json!({ "role": "system", "content": content }));
                }
                SystemMessageMode::Developer => {
                    items.push(json!({ "role": "developer", "content": content }));
                }
                SystemMessageMode::Remove => {
                    warnings.push(Warning::Unsupported {
                        feature: "system messages".into(),
                        details: Some(
                            "System messages are not supported for this model and were removed"
                                .into(),
                        ),
                    });
                }
            },

            LanguageModelV4Message::User {
                content,
                provider_options: _,
            } => {
                let parts = convert_user_parts(content);
                items.push(json!({
                    "role": "user",
                    "content": parts,
                }));
            }

            LanguageModelV4Message::Assistant {
                content,
                provider_options: _,
            } => {
                convert_assistant_parts(content, &mut items);
            }

            LanguageModelV4Message::Tool {
                content,
                provider_options: _,
            } => {
                convert_tool_parts(content, &mut items);
            }
        }
    }

    (items, warnings)
}

fn convert_user_parts(parts: &[UserContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|part| match part {
            UserContentPart::Text(text_part) => {
                json!({ "type": "input_text", "text": text_part.text })
            }
            UserContentPart::File(file_part) => {
                let media_type = &file_part.media_type;
                if media_type.starts_with("image/") {
                    let url = data_content_to_url(&file_part.data, media_type);
                    json!({ "type": "input_image", "image_url": url })
                } else {
                    // Generic file
                    let b64 = data_content_to_base64(&file_part.data);
                    json!({
                        "type": "input_file",
                        "file_data": format!("data:{media_type};base64,{b64}"),
                    })
                }
            }
        })
        .collect()
}

fn convert_assistant_parts(parts: &[AssistantContentPart], items: &mut Vec<Value>) {
    // Collect text parts into a message, and emit tool calls as separate items
    let mut text_parts = Vec::new();
    for part in parts {
        match part {
            AssistantContentPart::Text(tp) => {
                text_parts.push(json!({ "type": "output_text", "text": tp.text }));
            }
            AssistantContentPart::ToolCall(tc) => {
                // Flush text first
                if !text_parts.is_empty() {
                    items.push(json!({
                        "role": "assistant",
                        "content": text_parts.clone(),
                    }));
                    text_parts.clear();
                }
                items.push(json!({
                    "type": "function_call",
                    "call_id": tc.tool_call_id,
                    "name": tc.tool_name,
                    "arguments": serde_json::to_string(&tc.input).unwrap_or_default(),
                }));
            }
            AssistantContentPart::Reasoning(rp) => {
                // Flush text first
                if !text_parts.is_empty() {
                    items.push(json!({
                        "role": "assistant",
                        "content": text_parts.clone(),
                    }));
                    text_parts.clear();
                }
                items.push(json!({
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": rp.text }],
                }));
            }
            _ => {
                // Source, File, ToolResult, ToolApprovalRequest — skip or handle as needed
            }
        }
    }

    // Flush remaining text
    if !text_parts.is_empty() {
        items.push(json!({
            "role": "assistant",
            "content": text_parts,
        }));
    }
}

fn convert_tool_parts(parts: &[ToolContentPart], items: &mut Vec<Value>) {
    for part in parts {
        match part {
            ToolContentPart::ToolResult(result) => {
                let output = serialize_tool_result_for_responses(&result.output);
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": result.tool_call_id,
                    "output": output,
                }));
            }
            ToolContentPart::ToolApprovalResponse(apr) => {
                items.push(json!({
                    "type": "mcp_approval_response",
                    "approval_request_id": apr.approval_id,
                    "approve": apr.approved,
                }));
            }
        }
    }
}

fn serialize_tool_result_for_responses(content: &ToolResultContent) -> Value {
    match content {
        ToolResultContent::Text { value, .. } => Value::String(value.clone()),
        ToolResultContent::Json { value, .. } => {
            Value::String(serde_json::to_string(value).unwrap_or_default())
        }
        ToolResultContent::ErrorText { value, .. } => Value::String(value.clone()),
        ToolResultContent::ErrorJson { value, .. } => {
            Value::String(serde_json::to_string(value).unwrap_or_default())
        }
        ToolResultContent::ExecutionDenied { reason, .. } => {
            Value::String(reason.clone().unwrap_or_else(|| "Execution denied".into()))
        }
        ToolResultContent::Content { value, .. } => {
            let parts: Vec<Value> = value
                .iter()
                .filter_map(|p| match p {
                    ToolResultContentPart::Text { text, .. } => {
                        Some(json!({ "type": "input_text", "text": text }))
                    }
                    ToolResultContentPart::ImageUrl { url, .. } => {
                        Some(json!({ "type": "input_image", "image_url": url }))
                    }
                    ToolResultContentPart::ImageData {
                        data, media_type, ..
                    } => Some(
                        json!({ "type": "input_image", "image_url": format!("data:{media_type};base64,{data}") }),
                    ),
                    _ => None,
                })
                .collect();
            Value::Array(parts)
        }
    }
}

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

fn data_content_to_base64(data: &DataContent) -> String {
    match data {
        DataContent::Base64(b64) => b64.clone(),
        DataContent::Bytes(bytes) => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        DataContent::Url(url) => {
            if let Some(idx) = url.find(";base64,") {
                url[idx + 8..].to_string()
            } else {
                url.clone()
            }
        }
    }
}

#[cfg(test)]
#[path = "convert_to_responses_input.test.rs"]
mod tests;
