//! Convert Vercel AI SDK messages to Google Generative AI format.

use base64::Engine as _;
use serde_json::Value;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::DataContent;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::ToolContentPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::UserContentPart;

use crate::google_generative_ai_prompt::FileDataPart;
use crate::google_generative_ai_prompt::FunctionCallPart;
use crate::google_generative_ai_prompt::FunctionResponsePart;
use crate::google_generative_ai_prompt::GoogleContentRole;
use crate::google_generative_ai_prompt::GoogleGenerativeAIContent;
use crate::google_generative_ai_prompt::GoogleGenerativeAIContentPart;
use crate::google_generative_ai_prompt::GoogleGenerativeAIPrompt;
use crate::google_generative_ai_prompt::GoogleGenerativeAISystemInstruction;
use crate::google_generative_ai_prompt::GoogleTextPart;
use crate::google_generative_ai_prompt::InlineDataPart;
use crate::google_supported_file_url::is_supported_file_url;

/// Options for message conversion.
pub struct ConvertOptions {
    /// Whether the model supports system instructions (Gemma models don't).
    pub supports_system_instruction: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            supports_system_instruction: true,
        }
    }
}

/// Convert Vercel AI SDK messages to Google Generative AI prompt format.
pub fn convert_to_google_generative_ai_messages(
    prompt: &[LanguageModelV4Message],
    options: &ConvertOptions,
) -> GoogleGenerativeAIPrompt {
    let mut system_instruction_parts: Vec<GoogleTextPart> = Vec::new();
    let mut contents: Vec<GoogleGenerativeAIContent> = Vec::new();

    for message in prompt {
        match message {
            LanguageModelV4Message::System { content, .. } => {
                if options.supports_system_instruction {
                    system_instruction_parts.push(GoogleTextPart {
                        text: content.clone(),
                    });
                } else {
                    // For models like Gemma that don't support system instructions,
                    // prepend as a user message.
                    contents.push(GoogleGenerativeAIContent {
                        role: GoogleContentRole::User,
                        parts: vec![GoogleGenerativeAIContentPart::Text {
                            text: content.clone(),
                        }],
                    });
                }
            }
            LanguageModelV4Message::User { content, .. } => {
                let parts = convert_user_content_parts(content);
                if !parts.is_empty() {
                    contents.push(GoogleGenerativeAIContent {
                        role: GoogleContentRole::User,
                        parts,
                    });
                }
            }
            LanguageModelV4Message::Assistant { content, .. } => {
                let parts = convert_assistant_content_parts(content);
                if !parts.is_empty() {
                    contents.push(GoogleGenerativeAIContent {
                        role: GoogleContentRole::Model,
                        parts,
                    });
                }
            }
            LanguageModelV4Message::Tool { content, .. } => {
                let parts = convert_tool_content_parts(content);
                if !parts.is_empty() {
                    contents.push(GoogleGenerativeAIContent {
                        role: GoogleContentRole::User,
                        parts,
                    });
                }
            }
        }
    }

    let system_instruction = if system_instruction_parts.is_empty() {
        None
    } else {
        Some(GoogleGenerativeAISystemInstruction {
            parts: system_instruction_parts,
        })
    };

    GoogleGenerativeAIPrompt {
        system_instruction,
        contents,
    }
}

fn convert_user_content_parts(parts: &[UserContentPart]) -> Vec<GoogleGenerativeAIContentPart> {
    let mut result = Vec::new();
    for part in parts {
        match part {
            UserContentPart::Text(text_part) => {
                result.push(GoogleGenerativeAIContentPart::Text {
                    text: text_part.text.clone(),
                });
            }
            UserContentPart::File(file_part) => {
                result.push(convert_file_part(&file_part.data, &file_part.media_type));
            }
        }
    }
    result
}

fn convert_assistant_content_parts(
    parts: &[AssistantContentPart],
) -> Vec<GoogleGenerativeAIContentPart> {
    let mut result = Vec::new();
    for part in parts {
        match part {
            AssistantContentPart::Text(text_part) => {
                result.push(GoogleGenerativeAIContentPart::Text {
                    text: text_part.text.clone(),
                });
            }
            AssistantContentPart::File(file_part) => {
                result.push(convert_file_part(&file_part.data, &file_part.media_type));
            }
            AssistantContentPart::Reasoning(_) => {
                // Reasoning parts are not sent back to Google API.
            }
            AssistantContentPart::ToolCall(tool_call_part) => {
                result.push(GoogleGenerativeAIContentPart::FunctionCall {
                    function_call: FunctionCallPart {
                        name: tool_call_part.tool_name.clone(),
                        args: tool_call_part.input.clone(),
                    },
                });
            }
            AssistantContentPart::ToolResult(_) => {
                // Tool results in assistant messages are not sent to Google.
            }
            AssistantContentPart::Source(_) => {
                // Source parts are informational, not sent to Google.
            }
            AssistantContentPart::ToolApprovalRequest(_) => {
                // Tool approval requests are not sent to Google.
            }
        }
    }
    result
}

fn convert_tool_content_parts(parts: &[ToolContentPart]) -> Vec<GoogleGenerativeAIContentPart> {
    let mut result = Vec::new();
    for part in parts {
        match part {
            ToolContentPart::ToolResult(tool_result) => {
                let response_value = convert_tool_result_output(&tool_result.output);
                result.push(GoogleGenerativeAIContentPart::FunctionResponse {
                    function_response: FunctionResponsePart {
                        name: tool_result.tool_name.clone(),
                        response: response_value,
                    },
                });
            }
            ToolContentPart::ToolApprovalResponse(_) => {
                // Tool approval responses are not directly sent to Google.
            }
        }
    }
    result
}

fn convert_tool_result_output(output: &ToolResultContent) -> Value {
    match output {
        ToolResultContent::Text { value, .. } => {
            serde_json::json!({ "result": value })
        }
        ToolResultContent::Json { value, .. } => {
            serde_json::json!({ "result": value })
        }
        ToolResultContent::ExecutionDenied { reason, .. } => {
            let msg = reason.as_deref().unwrap_or("Tool execution was denied.");
            serde_json::json!({ "error": msg })
        }
        ToolResultContent::ErrorText { value, .. } => {
            serde_json::json!({ "error": value })
        }
        ToolResultContent::ErrorJson { value, .. } => {
            serde_json::json!({ "error": value })
        }
        ToolResultContent::Content { value, .. } => {
            // For multi-part content, serialize the parts as JSON.
            serde_json::json!({ "result": serde_json::to_value(value).unwrap_or(Value::Null) })
        }
    }
}

fn convert_file_part(data: &DataContent, media_type: &str) -> GoogleGenerativeAIContentPart {
    match data {
        DataContent::Url(url) => {
            if is_supported_file_url(url) {
                GoogleGenerativeAIContentPart::FileData {
                    file_data: FileDataPart {
                        mime_type: media_type.to_string(),
                        file_uri: url.clone(),
                    },
                }
            } else {
                // For non-Google URLs, send as file data (the API may reject unsupported URLs).
                GoogleGenerativeAIContentPart::FileData {
                    file_data: FileDataPart {
                        mime_type: media_type.to_string(),
                        file_uri: url.clone(),
                    },
                }
            }
        }
        DataContent::Base64(base64) => GoogleGenerativeAIContentPart::InlineData {
            inline_data: InlineDataPart {
                mime_type: media_type.to_string(),
                data: base64.clone(),
            },
        },
        DataContent::Bytes(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            GoogleGenerativeAIContentPart::InlineData {
                inline_data: InlineDataPart {
                    mime_type: media_type.to_string(),
                    data: encoded,
                },
            }
        }
    }
}

#[cfg(test)]
#[path = "convert_to_google_generative_ai_messages.test.rs"]
mod tests;
