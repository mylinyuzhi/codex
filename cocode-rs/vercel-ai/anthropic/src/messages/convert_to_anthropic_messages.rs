use std::collections::HashSet;

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

/// Result of converting a prompt to Anthropic messages.
pub struct ConvertedMessages {
    pub system: Option<Vec<Value>>,
    pub messages: Vec<Value>,
    pub warnings: Vec<Warning>,
    pub betas: HashSet<String>,
}

/// Convert a `LanguageModelV4Prompt` into Anthropic Messages API format.
///
/// Returns `(system, messages, warnings)` where system is a separate array of
/// text blocks (Anthropic uses a top-level `system` field, not a system role message).
pub fn convert_to_anthropic_messages(
    prompt: &LanguageModelV4Prompt,
    send_reasoning: bool,
) -> (Option<Vec<Value>>, Vec<Value>, Vec<Warning>) {
    let result = convert_to_anthropic_messages_full(prompt, send_reasoning);
    (result.system, result.messages, result.warnings)
}

/// Convert prompt with full result including betas.
pub fn convert_to_anthropic_messages_full(
    prompt: &LanguageModelV4Prompt,
    send_reasoning: bool,
) -> ConvertedMessages {
    let mut system_blocks: Vec<Value> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();
    let mut betas: HashSet<String> = HashSet::new();

    for msg in prompt {
        match msg {
            LanguageModelV4Message::System {
                content,
                provider_options: _,
            } => {
                system_blocks.push(json!({
                    "type": "text",
                    "text": content,
                }));
            }

            LanguageModelV4Message::User {
                content,
                provider_options: _,
            } => {
                let parts = convert_user_parts(content, &mut betas);
                if !parts.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": parts,
                    }));
                }
            }

            LanguageModelV4Message::Assistant {
                content,
                provider_options: _,
            } => {
                let parts = convert_assistant_parts(content, send_reasoning);
                if !parts.is_empty() {
                    messages.push(json!({
                        "role": "assistant",
                        "content": parts,
                    }));
                }
            }

            LanguageModelV4Message::Tool {
                content,
                provider_options: _,
            } => {
                let parts = convert_tool_parts(content, &mut warnings);
                if !parts.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": parts,
                    }));
                }
            }
        }
    }

    let system = if system_blocks.is_empty() {
        None
    } else {
        Some(system_blocks)
    };

    ConvertedMessages {
        system,
        messages,
        warnings,
        betas,
    }
}

/// Convert user content parts to Anthropic format.
fn convert_user_parts(parts: &[UserContentPart], betas: &mut HashSet<String>) -> Vec<Value> {
    parts
        .iter()
        .map(|part| match part {
            UserContentPart::Text(text_part) => {
                json!({
                    "type": "text",
                    "text": text_part.text,
                })
            }
            UserContentPart::File(file_part) => {
                let media_type = &file_part.media_type;
                if media_type.starts_with("image/") {
                    // Image content
                    let source = data_content_to_anthropic_source(&file_part.data, media_type);
                    json!({
                        "type": "image",
                        "source": source,
                    })
                } else if media_type == "application/pdf" {
                    // PDF document — add beta and extract provider options
                    betas.insert("pdfs-2024-09-25".to_string());
                    let source = data_content_to_anthropic_source(&file_part.data, media_type);
                    let mut doc = json!({
                        "type": "document",
                        "source": source,
                    });
                    // Extract provider options for PDF (citations, title, context)
                    if let Some(ref pm) = file_part.provider_metadata
                        && let Some(opts) = pm.0.get("anthropic")
                    {
                        if let Some(citations) = opts.get("citations") {
                            doc["citations"] = citations.clone();
                        }
                        if let Some(title) = opts.get("title") {
                            doc["title"] = title.clone();
                        }
                        if let Some(context) = opts.get("context") {
                            doc["context"] = context.clone();
                        }
                    }
                    doc
                } else if media_type.starts_with("text/") {
                    // Text document
                    let source = data_content_to_text_source(&file_part.data, media_type);
                    json!({
                        "type": "document",
                        "source": source,
                    })
                } else {
                    // Generic file — use base64 document
                    let source = data_content_to_anthropic_source(&file_part.data, media_type);
                    json!({
                        "type": "document",
                        "source": source,
                    })
                }
            }
        })
        .collect()
}

/// Convert assistant content parts to Anthropic format.
fn convert_assistant_parts(parts: &[AssistantContentPart], send_reasoning: bool) -> Vec<Value> {
    let mut result = Vec::new();

    for part in parts {
        match part {
            AssistantContentPart::Text(text_part) => {
                result.push(json!({
                    "type": "text",
                    "text": text_part.text,
                }));
            }
            AssistantContentPart::ToolCall(tc) => {
                result.push(json!({
                    "type": "tool_use",
                    "id": tc.tool_call_id,
                    "name": tc.tool_name,
                    "input": tc.input,
                }));
            }
            AssistantContentPart::Reasoning(reasoning) => {
                if send_reasoning {
                    // Check for redacted_thinking via provider_metadata
                    let is_redacted = reasoning
                        .provider_metadata
                        .as_ref()
                        .and_then(|pm| pm.0.get("anthropic"))
                        .and_then(|v| v.get("redactedData"))
                        .is_some();

                    if is_redacted {
                        let data = reasoning
                            .provider_metadata
                            .as_ref()
                            .and_then(|pm| pm.0.get("anthropic"))
                            .and_then(|v| v.get("redactedData"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        result.push(json!({
                            "type": "redacted_thinking",
                            "data": data,
                        }));
                    } else {
                        let signature = reasoning
                            .provider_metadata
                            .as_ref()
                            .and_then(|pm| pm.0.get("anthropic"))
                            .and_then(|v| v.get("signature"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        result.push(json!({
                            "type": "thinking",
                            "thinking": reasoning.text,
                            "signature": signature,
                        }));
                    }
                }
            }
            // Source, File, ToolResult, ToolApprovalRequest — skip
            _ => {}
        }
    }

    result
}

/// Convert tool content parts to Anthropic `tool_result` blocks.
fn convert_tool_parts(parts: &[ToolContentPart], warnings: &mut Vec<Warning>) -> Vec<Value> {
    let mut result = Vec::new();

    for part in parts {
        match part {
            ToolContentPart::ToolResult(tool_result) => {
                let (content, is_error) = serialize_tool_result(&tool_result.output);
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_result.tool_call_id,
                    "content": content,
                });
                if is_error {
                    block["is_error"] = Value::Bool(true);
                }
                result.push(block);
            }
            ToolContentPart::ToolApprovalResponse(_) => {
                warnings.push(Warning::Unsupported {
                    feature: "tool approval responses".into(),
                    details: Some(
                        "Tool approval responses are not supported in Anthropic Messages API"
                            .into(),
                    ),
                });
            }
        }
    }

    result
}

/// Serialize tool result content into `(Value, is_error)`.
fn serialize_tool_result(content: &ToolResultContent) -> (Value, bool) {
    match content {
        ToolResultContent::Text { value, .. } => (Value::String(value.clone()), false),
        ToolResultContent::Json { value, .. } => (
            Value::String(serde_json::to_string(value).unwrap_or_default()),
            false,
        ),
        ToolResultContent::ErrorText { value, .. } => (Value::String(value.clone()), true),
        ToolResultContent::ErrorJson { value, .. } => (
            Value::String(serde_json::to_string(value).unwrap_or_default()),
            true,
        ),
        ToolResultContent::ExecutionDenied { reason, .. } => {
            let msg = reason.clone().unwrap_or_else(|| "Execution denied".into());
            (Value::String(msg), true)
        }
        ToolResultContent::Content { value, .. } => {
            let parts: Vec<Value> = value
                .iter()
                .filter_map(|part| match part {
                    vercel_ai_provider::ToolResultContentPart::Text { text, .. } => {
                        Some(json!({"type": "text", "text": text}))
                    }
                    vercel_ai_provider::ToolResultContentPart::FileData {
                        data,
                        media_type,
                        ..
                    } => {
                        let source = data_content_to_anthropic_source(
                            &DataContent::Base64(data.clone()),
                            media_type,
                        );
                        Some(json!({"type": "image", "source": source}))
                    }
                    vercel_ai_provider::ToolResultContentPart::FileUrl { .. } => {
                        tracing::warn!(
                            "FileUrl tool result content parts are not supported in the Anthropic API"
                        );
                        None
                    }
                    _ => None,
                })
                .collect();
            (Value::Array(parts), false)
        }
    }
}

/// Convert DataContent to an Anthropic source object (`base64` or `url`).
fn data_content_to_anthropic_source(data: &DataContent, media_type: &str) -> Value {
    match data {
        DataContent::Url(url) => {
            json!({
                "type": "url",
                "url": url,
            })
        }
        DataContent::Base64(b64) => {
            json!({
                "type": "base64",
                "media_type": media_type,
                "data": b64,
            })
        }
        DataContent::Bytes(bytes) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            json!({
                "type": "base64",
                "media_type": media_type,
                "data": b64,
            })
        }
    }
}

/// Convert DataContent to a text source for text/* documents.
fn data_content_to_text_source(data: &DataContent, media_type: &str) -> Value {
    match data {
        DataContent::Base64(b64) => {
            // Try to decode the base64 to get the text
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                if let Ok(text) = String::from_utf8(bytes) {
                    return json!({
                        "type": "text",
                        "media_type": media_type,
                        "data": text,
                    });
                }
            }
            // Fall back to base64
            json!({
                "type": "base64",
                "media_type": media_type,
                "data": b64,
            })
        }
        DataContent::Bytes(bytes) => {
            if let Ok(text) = String::from_utf8(bytes.clone()) {
                json!({
                    "type": "text",
                    "media_type": media_type,
                    "data": text,
                })
            } else {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                json!({
                    "type": "base64",
                    "media_type": media_type,
                    "data": b64,
                })
            }
        }
        DataContent::Url(url) => {
            json!({
                "type": "url",
                "url": url,
            })
        }
    }
}

#[cfg(test)]
#[path = "convert_to_anthropic_messages.test.rs"]
mod tests;
