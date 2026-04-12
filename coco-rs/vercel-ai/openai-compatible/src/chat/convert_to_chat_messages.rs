use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::DataContent;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::ProviderMetadata;
use vercel_ai_provider::ToolContentPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::Warning;

/// Convert a `LanguageModelV4Prompt` into OpenAI-compatible Chat Completions API messages.
///
/// Unlike the OpenAI-specific converter, this always uses `role: "system"` (no Developer mode)
/// and includes `reasoning_content` in assistant messages.
///
/// Returns `Ok((messages, warnings))`.
pub fn convert_to_openai_compatible_chat_messages(
    prompt: &LanguageModelV4Prompt,
) -> Result<(Vec<Value>, Vec<Warning>), AISdkError> {
    let mut messages = Vec::new();
    let warnings = Vec::new();

    for msg in prompt {
        match msg {
            LanguageModelV4Message::System {
                content,
                provider_options,
            } => {
                let mut msg = json!({ "role": "system", "content": content });
                for (k, v) in get_openai_metadata(provider_options) {
                    msg[k] = v;
                }
                messages.push(msg);
            }

            LanguageModelV4Message::User {
                content,
                provider_options,
            } => {
                let parts = convert_user_parts(content, provider_options)?;
                // Single text part can be simplified to just a string
                if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
                {
                    let mut msg = json!({
                        "role": "user",
                        "content": parts[0]["text"]
                    });
                    // For single text, spread the part's metadata on the message
                    if let Some(UserContentPart::Text(text_part)) = content.first() {
                        for (k, v) in get_part_metadata(&text_part.provider_metadata) {
                            msg[k] = v;
                        }
                    }
                    messages.push(msg);
                } else {
                    let mut msg = json!({
                        "role": "user",
                        "content": parts
                    });
                    // For multi-part, spread message-level metadata on the message
                    for (k, v) in get_openai_metadata(provider_options) {
                        msg[k] = v;
                    }
                    messages.push(msg);
                }
            }

            LanguageModelV4Message::Assistant {
                content,
                provider_options,
            } => {
                let (text, tool_calls, reasoning_content) = convert_assistant_parts(content);
                let mut msg = json!({ "role": "assistant" });
                if let Some(text) = text {
                    msg["content"] = Value::String(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = Value::Array(tool_calls);
                }
                // Include reasoning_content in assistant messages for providers that support it
                if let Some(reasoning) = reasoning_content {
                    msg["reasoning_content"] = Value::String(reasoning);
                }
                for (k, v) in get_openai_metadata(provider_options) {
                    msg[k] = v;
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
                            let mut msg = json!({
                                "role": "tool",
                                "tool_call_id": result.tool_call_id,
                                "content": output,
                            });
                            for (k, v) in get_part_metadata(&result.provider_metadata) {
                                msg[k] = v;
                            }
                            messages.push(msg);
                        }
                        ToolContentPart::ToolApprovalResponse(_) => {
                            // Approval responses are not supported in Chat API
                        }
                    }
                }
            }
        }
    }

    Ok((messages, warnings))
}

/// Extract provider metadata from the "openaiCompatible" key in provider options.
fn get_openai_metadata(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> serde_json::Map<String, Value> {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openaiCompatible"))
        .map(|inner| inner.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

/// Extract provider metadata from the "openaiCompatible" key in a content part's provider metadata.
fn get_part_metadata(pm: &Option<ProviderMetadata>) -> serde_json::Map<String, Value> {
    pm.as_ref()
        .and_then(|meta| meta.0.get("openaiCompatible"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}

/// Convert user content parts to OpenAI-compatible format.
fn convert_user_parts(
    parts: &[UserContentPart],
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> Result<Vec<Value>, AISdkError> {
    // Extract imageDetail from any provider options (generic key lookup)
    let image_detail = provider_options.as_ref().and_then(|opts| {
        // Try to find imageDetail in any provider's options
        opts.0.values().find_map(|v| {
            v.get("imageDetail")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
    });

    parts
        .iter()
        .map(|part| match part {
            UserContentPart::Text(text_part) => {
                let mut val = json!({ "type": "text", "text": text_part.text });
                for (k, v) in get_part_metadata(&text_part.provider_metadata) {
                    val[k] = v;
                }
                Ok(val)
            }
            UserContentPart::File(file_part) => {
                let media_type = &file_part.media_type;
                let part_meta = get_part_metadata(&file_part.provider_metadata);

                if media_type.starts_with("image/") {
                    // #16: Convert image/* to image/jpeg as fallback
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
                    let mut val = json!({ "type": "image_url", "image_url": image_url });
                    for (k, v) in part_meta {
                        val[k] = v;
                    }
                    Ok(val)
                } else if media_type.starts_with("audio/") {
                    // Audio parts with URLs are not supported
                    if matches!(file_part.data, DataContent::Url(_)) {
                        return Err(AISdkError::new(
                            "Unsupported functionality: audio file parts with URLs",
                        ));
                    }
                    let format = match media_type.as_str() {
                        "audio/wav" => "wav",
                        "audio/mp3" | "audio/mpeg" => "mp3",
                        _ => {
                            return Err(AISdkError::new(format!(
                                "Unsupported functionality: audio media type {media_type}"
                            )));
                        }
                    };
                    let b64 = data_content_to_base64(&file_part.data);
                    let mut val = json!({
                        "type": "input_audio",
                        "input_audio": { "data": b64, "format": format }
                    });
                    for (k, v) in part_meta {
                        val[k] = v;
                    }
                    Ok(val)
                } else if media_type == "application/pdf" {
                    // PDF parts with URLs are not supported
                    if matches!(file_part.data, DataContent::Url(_)) {
                        return Err(AISdkError::new(
                            "Unsupported functionality: PDF file parts with URLs",
                        ));
                    }
                    let b64 = data_content_to_base64(&file_part.data);
                    let mut val = json!({
                        "type": "file",
                        "file": {
                            "filename": "document.pdf",
                            "file_data": format!("data:{media_type};base64,{b64}"),
                        }
                    });
                    for (k, v) in part_meta {
                        val[k] = v;
                    }
                    Ok(val)
                } else if media_type.starts_with("text/") {
                    let text = decode_text_data(&file_part.data);
                    let mut val = json!({ "type": "text", "text": text });
                    for (k, v) in part_meta {
                        val[k] = v;
                    }
                    Ok(val)
                } else {
                    Err(AISdkError::new(format!(
                        "Unsupported functionality: file part media type {media_type}"
                    )))
                }
            }
        })
        .collect()
}

/// Convert assistant content parts to (concatenated text, tool_calls array, reasoning_content).
fn convert_assistant_parts(
    parts: &[AssistantContentPart],
) -> (Option<String>, Vec<Value>, Option<String>) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_parts = Vec::new();

    for part in parts {
        match part {
            AssistantContentPart::Text(text_part) => {
                text_parts.push(text_part.text.clone());
            }
            AssistantContentPart::ToolCall(tc) => {
                let mut tool_call = json!({
                    "id": tc.tool_call_id,
                    "type": "function",
                    "function": {
                        "name": tc.tool_name,
                        "arguments": serde_json::to_string(&tc.input).unwrap_or_default(),
                    }
                });

                // Spread openaiCompatible part metadata
                for (k, v) in get_part_metadata(&tc.provider_metadata) {
                    tool_call[k] = v;
                }

                // #4: Include thought_signature as extra_content for Google
                // (after partMetadata so it overrides if conflicting)
                if let Some(ref pm) = tc.provider_metadata
                    && let Some(google) = pm.0.get("google")
                    && let Some(ts) = google.get("thoughtSignature").and_then(|v| v.as_str())
                {
                    tool_call["extra_content"] = json!({
                        "google": {
                            "thought_signature": ts
                        }
                    });
                }

                tool_calls.push(tool_call);
            }
            AssistantContentPart::Reasoning(rp) => {
                reasoning_parts.push(rp.text.clone());
            }
            // File, Source, ToolResult, ToolApprovalRequest — skip
            _ => {}
        }
    }

    let text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    let reasoning = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join(""))
    };

    (text, tool_calls, reasoning)
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
        ToolResultContent::ExecutionDenied { reason, .. } => {
            reason.clone().unwrap_or_else(|| "Execution denied".into())
        }
        ToolResultContent::Content { value, .. } => {
            // Serialize the full content value as JSON (matches TS JSON.stringify behavior)
            serde_json::to_string(value).unwrap_or_default()
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

/// Decode DataContent as UTF-8 text (for text/* media types).
fn decode_text_data(data: &DataContent) -> String {
    match data {
        DataContent::Url(url) => url.clone(),
        DataContent::Base64(b64) => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        }
        DataContent::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
#[path = "convert_to_chat_messages.test.rs"]
mod tests;
