use std::collections::HashSet;

use serde_json::Value;
use serde_json::json;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::DataContent;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::ToolContentPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::ToolResultContentPart;
use vercel_ai_provider::UserContentPart;
use vercel_ai_provider::Warning;

use crate::openai_capabilities::SystemMessageMode;

/// Flags indicating which provider tools are present, used for input conversion.
#[derive(Default)]
pub struct ProviderToolFlags {
    pub has_local_shell: bool,
    pub has_shell: bool,
    pub has_apply_patch: bool,
    pub custom_tool_names: HashSet<String>,
}

impl ProviderToolFlags {
    /// Detect provider tool flags from the tools list.
    pub fn from_tools(tools: &Option<Vec<LanguageModelV4Tool>>) -> Self {
        let Some(tools) = tools else {
            return Self::default();
        };
        let mut flags = Self::default();
        let builtin_types = [
            "file_search",
            "web_search",
            "web_search_preview",
            "code_interpreter",
            "shell",
            "local_shell",
            "apply_patch",
            "image_generation",
            "mcp",
        ];
        for tool in tools {
            if let LanguageModelV4Tool::Provider(pt) = tool {
                match pt.name.as_str() {
                    "local_shell" => flags.has_local_shell = true,
                    "shell" => flags.has_shell = true,
                    "apply_patch" => flags.has_apply_patch = true,
                    name if !builtin_types.contains(&name) => {
                        flags.custom_tool_names.insert(name.to_string());
                    }
                    _ => {}
                }
            }
        }
        flags
    }
}

/// Convert a `LanguageModelV4Prompt` into OpenAI Responses API input items.
///
/// Returns `(input_items, warnings)`.
pub fn convert_to_openai_responses_input(
    prompt: &LanguageModelV4Prompt,
    system_message_mode: SystemMessageMode,
) -> (Vec<Value>, Vec<Warning>) {
    convert_to_openai_responses_input_with_flags(
        prompt,
        system_message_mode,
        &ProviderToolFlags::default(),
    )
}

/// Convert with provider tool flags for proper tool type handling.
pub fn convert_to_openai_responses_input_with_flags(
    prompt: &LanguageModelV4Prompt,
    system_message_mode: SystemMessageMode,
    flags: &ProviderToolFlags,
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
                convert_assistant_parts(content, &mut items, flags);
            }

            LanguageModelV4Message::Tool {
                content,
                provider_options: _,
            } => {
                convert_tool_parts(content, &mut items, flags);
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
                    // Convert wildcard image/* to image/jpeg
                    let effective_type = if media_type == "image/*" {
                        "image/jpeg"
                    } else {
                        media_type.as_str()
                    };
                    // Check for file ID
                    if let DataContent::Base64(ref s) = file_part.data
                        && s.starts_with("file-")
                    {
                        return json!({ "type": "input_image", "file_id": s });
                    }
                    let url = data_content_to_url(&file_part.data, effective_type);
                    json!({ "type": "input_image", "image_url": url })
                } else if media_type == "application/pdf" {
                    // Check for file ID
                    if let DataContent::Base64(ref s) = file_part.data
                        && s.starts_with("file-")
                    {
                        return json!({ "type": "input_file", "file_id": s });
                    }
                    let b64 = data_content_to_base64(&file_part.data);
                    json!({
                        "type": "input_file",
                        "file_data": format!("data:{media_type};base64,{b64}"),
                    })
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

fn convert_assistant_parts(
    parts: &[AssistantContentPart],
    items: &mut Vec<Value>,
    flags: &ProviderToolFlags,
) {
    // Collect text parts into a message, and emit tool calls as separate items
    let mut text_parts = Vec::new();
    let flush_text = |text_parts: &mut Vec<Value>, items: &mut Vec<Value>| {
        if !text_parts.is_empty() {
            items.push(json!({
                "role": "assistant",
                "content": text_parts.clone(),
            }));
            text_parts.clear();
        }
    };

    for part in parts {
        match part {
            AssistantContentPart::Text(tp) => {
                text_parts.push(json!({ "type": "output_text", "text": tp.text }));
            }
            AssistantContentPart::ToolCall(tc) => {
                flush_text(&mut text_parts, items);

                // Skip provider-executed tool calls (they are already in the context)
                if tc.provider_executed == Some(true) {
                    continue;
                }

                let tool_name = &tc.tool_name;

                // Handle provider-specific tool call types
                if flags.has_local_shell && tool_name == "local_shell" {
                    // Local shell tool calls use a specific format
                    items.push(json!({
                        "type": "local_shell_call",
                        "call_id": tc.tool_call_id,
                        "action": tc.input,
                    }));
                } else if flags.has_shell && tool_name == "shell" {
                    items.push(json!({
                        "type": "shell_call",
                        "call_id": tc.tool_call_id,
                        "status": "completed",
                        "action": tc.input,
                    }));
                } else if flags.has_apply_patch && tool_name == "apply_patch" {
                    items.push(json!({
                        "type": "apply_patch_call",
                        "call_id": tc.tool_call_id,
                        "status": "completed",
                        "operation": tc.input,
                    }));
                } else if flags.custom_tool_names.contains(tool_name) {
                    let input_str = if tc.input.is_string() {
                        tc.input.as_str().unwrap_or("").to_string()
                    } else {
                        serde_json::to_string(&tc.input).unwrap_or_default()
                    };
                    items.push(json!({
                        "type": "custom_tool_call",
                        "call_id": tc.tool_call_id,
                        "name": tool_name,
                        "input": input_str,
                    }));
                } else {
                    items.push(json!({
                        "type": "function_call",
                        "call_id": tc.tool_call_id,
                        "name": tool_name,
                        "arguments": serde_json::to_string(&tc.input).unwrap_or_default(),
                    }));
                }
            }
            AssistantContentPart::Reasoning(rp) => {
                flush_text(&mut text_parts, items);
                items.push(json!({
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": rp.text }],
                }));
            }
            _ => {
                // Source, File, ToolResult, ToolApprovalRequest — skip
            }
        }
    }

    // Flush remaining text
    flush_text(&mut text_parts, items);
}

fn convert_tool_parts(
    parts: &[ToolContentPart],
    items: &mut Vec<Value>,
    flags: &ProviderToolFlags,
) {
    for part in parts {
        match part {
            ToolContentPart::ToolResult(result) => {
                // Skip execution-denied results
                if matches!(&result.output, ToolResultContent::ExecutionDenied { .. }) {
                    continue;
                }

                let tool_name = result.tool_name.as_str();

                // Handle provider-specific tool output types
                if flags.has_local_shell
                    && tool_name == "local_shell"
                    && let ToolResultContent::Json { value, .. } = &result.output
                {
                    items.push(json!({
                        "type": "local_shell_call_output",
                        "call_id": result.tool_call_id,
                        "output": value,
                    }));
                    continue;
                }

                if flags.has_shell
                    && tool_name == "shell"
                    && let ToolResultContent::Json { value, .. } = &result.output
                {
                    items.push(json!({
                        "type": "shell_call_output",
                        "call_id": result.tool_call_id,
                        "output": value,
                    }));
                    continue;
                }

                if flags.has_apply_patch
                    && tool_name == "apply_patch"
                    && let ToolResultContent::Json { value, .. } = &result.output
                {
                    items.push(json!({
                        "type": "apply_patch_call_output",
                        "call_id": result.tool_call_id,
                        "output": value,
                    }));
                    continue;
                }

                if flags.custom_tool_names.contains(tool_name) {
                    let output = serialize_tool_result_for_responses(&result.output);
                    items.push(json!({
                        "type": "custom_tool_call_output",
                        "call_id": result.tool_call_id,
                        "output": output,
                    }));
                    continue;
                }

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
        ToolResultContent::ExecutionDenied { reason, .. } => Value::String(
            reason
                .clone()
                .unwrap_or_else(|| "Tool execution denied.".into()),
        ),
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
