//! Conversion functions between codex-api and Anthropic SDK types.
//!
//! This module stores the full `Message` response in `Reasoning.encrypted_content`
//! for round-trip preservation. On sendback, we extract the Content directly
//! from the stored response.

use std::collections::HashSet;

use anthropic_sdk::ContentBlock;
use anthropic_sdk::ContentBlockParam;
use anthropic_sdk::ImageMediaType;
use anthropic_sdk::Message;
use anthropic_sdk::MessageParam;
use anthropic_sdk::SystemPrompt;
use anthropic_sdk::Tool;
use anthropic_sdk::ToolResultContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;

use crate::common::Prompt;
use crate::common::ResponseEvent;
use crate::common_ext::EncryptedContent;
use crate::common_ext::PROVIDER_SDK_ANTHROPIC;

// ============================================================================
// Request conversion: Prompt -> Anthropic messages
// ============================================================================

/// Convert a codex-api Prompt to Anthropic MessageParams and optional SystemPrompt.
///
/// This function handles the conversion of:
/// - Reasoning with encrypted_content -> Extract Content directly from stored response
/// - User messages -> MessageParam with role="user"
/// - Assistant messages -> MessageParam with role="assistant" (skipped if already processed)
/// - FunctionCall -> Skipped if already processed, otherwise appended as ToolUse
/// - FunctionCallOutput -> MessageParam with ToolResult content
pub fn prompt_to_messages(prompt: &Prompt) -> (Vec<MessageParam>, Option<SystemPrompt>) {
    let mut messages: Vec<MessageParam> = Vec::new();
    let mut current_assistant_content: Vec<ContentBlockParam> = Vec::new();
    let mut processed_response_ids: HashSet<String> = HashSet::new();

    for item in &prompt.input {
        match item {
            // Handle Reasoning with stored full response first
            ResponseItem::Reasoning {
                id: resp_id,
                encrypted_content: Some(enc),
                ..
            } => {
                if processed_response_ids.contains(resp_id) {
                    continue;
                }
                // Flush any pending assistant content first
                flush_assistant_message(&mut messages, &mut current_assistant_content);

                if let Some(assistant_msg) = extract_full_response_message(enc) {
                    messages.push(assistant_msg);
                    processed_response_ids.insert(resp_id.clone());
                }
            }

            // Reasoning without encrypted_content - skip (handled by ThinkingConfig)
            ResponseItem::Reasoning { .. } => {}

            // Skip assistant messages if already processed via Reasoning
            ResponseItem::Message {
                id: Some(resp_id),
                role,
                ..
            } if role == "assistant" && processed_response_ids.contains(resp_id) => {
                continue;
            }

            // Skip FunctionCall if already processed via Reasoning
            ResponseItem::FunctionCall {
                id: Some(resp_id), ..
            } if processed_response_ids.contains(resp_id) => {
                continue;
            }

            ResponseItem::Message { role, content, .. } => {
                if role == "assistant" {
                    // Continue or start assistant message
                    current_assistant_content.extend(content.iter().map(content_item_to_block));
                } else {
                    // Flush any pending assistant message first
                    flush_assistant_message(&mut messages, &mut current_assistant_content);

                    // Add user message
                    let blocks: Vec<ContentBlockParam> =
                        content.iter().map(content_item_to_block).collect();
                    if !blocks.is_empty() {
                        messages.push(MessageParam::user_with_content(blocks));
                    }
                }
            }

            ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                // FunctionCall must be part of an assistant message
                let input: Value = serde_json::from_str(arguments).unwrap_or(Value::Object(
                    serde_json::Map::from_iter([(
                        "raw".to_string(),
                        Value::String(arguments.clone()),
                    )]),
                ));
                current_assistant_content.push(ContentBlockParam::ToolUse {
                    id: call_id.clone(),
                    name: name.clone(),
                    input,
                });
            }

            ResponseItem::FunctionCallOutput { call_id, output } => {
                // Flush assistant message first (tool result must follow tool_use)
                flush_assistant_message(&mut messages, &mut current_assistant_content);

                // Add tool result as user message
                let content = function_output_to_tool_result(call_id, output);
                messages.push(MessageParam::user_with_content(vec![content]));
            }

            // Skip types not applicable to Anthropic API:
            // LocalShellCall, CustomToolCall, WebSearchCall, GhostSnapshot, Compaction
            // These are handled by other parts of the system
            _ => {}
        }
    }

    // Flush any remaining assistant content
    flush_assistant_message(&mut messages, &mut current_assistant_content);

    // Extract system prompt
    let system_prompt = if prompt.instructions.is_empty() {
        None
    } else {
        Some(SystemPrompt::Text(prompt.instructions.clone()))
    };

    (messages, system_prompt)
}

/// Extract MessageParam from stored Anthropic Message body.
fn extract_full_response_message(encrypted_content: &str) -> Option<MessageParam> {
    let ec = EncryptedContent::from_json_string(encrypted_content)?;
    let message: Message = ec.parse_body()?;

    // Convert ContentBlock -> ContentBlockParam
    let content: Vec<ContentBlockParam> = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(ContentBlockParam::text(text)),
            ContentBlock::ToolUse { id, name, input } => Some(ContentBlockParam::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            // Skip thinking blocks - they are handled by ThinkingConfig
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => None,
        })
        .collect();

    if content.is_empty() {
        None
    } else {
        Some(MessageParam::assistant_with_content(content))
    }
}

/// Flush the current assistant message content to the messages list.
fn flush_assistant_message(
    messages: &mut Vec<MessageParam>,
    current_content: &mut Vec<ContentBlockParam>,
) {
    if !current_content.is_empty() {
        messages.push(MessageParam::assistant_with_content(std::mem::take(
            current_content,
        )));
    }
}

/// Convert a ContentItem to an Anthropic ContentBlockParam.
fn content_item_to_block(item: &ContentItem) -> ContentBlockParam {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
            ContentBlockParam::text(text)
        }
        ContentItem::InputImage { image_url } => parse_image_url_to_block(image_url),
    }
}

/// Parse a MIME type string to an Anthropic ImageMediaType.
fn parse_media_type(mime_str: &str) -> ImageMediaType {
    if mime_str.contains("image/png") {
        ImageMediaType::Png
    } else if mime_str.contains("image/jpeg") {
        ImageMediaType::Jpeg
    } else if mime_str.contains("image/gif") {
        ImageMediaType::Gif
    } else if mime_str.contains("image/webp") {
        ImageMediaType::Webp
    } else {
        ImageMediaType::Png
    }
}

/// Parse an image URL to an Anthropic ImageSource.
fn parse_image_source(image_url: &str) -> anthropic_sdk::ImageSource {
    if let Some(data_url) = image_url.strip_prefix("data:") {
        if let Some((mime_and_encoding, data)) = data_url.split_once(',') {
            let media_type = parse_media_type(mime_and_encoding);
            return anthropic_sdk::ImageSource::Base64 {
                data: data.to_string(),
                media_type,
            };
        }
    }
    anthropic_sdk::ImageSource::Url {
        url: image_url.to_string(),
    }
}

/// Parse an image URL (data URL or regular URL) to an Anthropic ContentBlockParam.
fn parse_image_url_to_block(image_url: &str) -> ContentBlockParam {
    let source = parse_image_source(image_url);
    ContentBlockParam::Image {
        source,
        cache_control: None,
    }
}

/// Convert FunctionCallOutput to a ToolResult ContentBlockParam.
fn function_output_to_tool_result(
    call_id: &str,
    output: &FunctionCallOutputPayload,
) -> ContentBlockParam {
    let is_error = output.success == Some(false);

    // Check if we have content_items for multimodal output
    if let Some(items) = &output.content_items {
        let blocks: Vec<anthropic_sdk::ToolResultContentBlock> = items
            .iter()
            .filter_map(|item| match item {
                codex_protocol::models::FunctionCallOutputContentItem::InputText { text } => {
                    Some(anthropic_sdk::ToolResultContentBlock::Text { text: text.clone() })
                }
                codex_protocol::models::FunctionCallOutputContentItem::InputImage { image_url } => {
                    Some(anthropic_sdk::ToolResultContentBlock::Image {
                        source: parse_image_source(image_url),
                    })
                }
            })
            .collect();

        if !blocks.is_empty() {
            return ContentBlockParam::ToolResult {
                tool_use_id: call_id.to_string(),
                content: Some(ToolResultContent::Blocks(blocks)),
                is_error: if is_error { Some(true) } else { None },
                cache_control: None,
            };
        }
    }

    // Simple text result
    if is_error {
        ContentBlockParam::tool_result_error(call_id, &output.content)
    } else {
        ContentBlockParam::tool_result(call_id, &output.content)
    }
}

// ============================================================================
// Tool conversion: JSON -> Anthropic Tool
// ============================================================================

/// Convert JSON tool definitions to Anthropic Tool structs.
///
/// Supports both OpenAI-style format:
/// ```json
/// {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
/// ```
/// And direct function format:
/// ```json
/// {"name": "...", "description": "...", "parameters": {...}}
/// ```
pub fn tools_to_anthropic(tools: &[Value]) -> Vec<Tool> {
    tools
        .iter()
        .filter_map(|tool| {
            // Try OpenAI-style format first
            if let Some(func) = tool.get("function") {
                return tool_json_to_struct(func);
            }
            // Try direct format
            tool_json_to_struct(tool)
        })
        .collect()
}

/// Convert a single tool JSON to an Anthropic Tool struct.
fn tool_json_to_struct(json: &Value) -> Option<Tool> {
    let name = json.get("name")?.as_str()?;
    let description = json
        .get("description")
        .and_then(|d| d.as_str())
        .map(String::from);
    let input_schema = json
        .get("parameters")
        .or_else(|| json.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            })
        });

    // Use Tool struct directly (skip validation since tools come from config)
    Some(Tool {
        name: name.to_string(),
        description,
        input_schema,
        cache_control: None,
    })
}

// ============================================================================
// Response conversion: Anthropic Message -> ResponseEvents
// ============================================================================

/// Convert an Anthropic Message response to codex-api ResponseEvents.
///
/// Returns a vector of events and optional token usage.
/// The events include:
/// - Created (response start)
/// - OutputItemDone for each content block (Message, FunctionCall, Reasoning)
/// - Completed (response end with usage)
pub fn message_to_events(message: &Message) -> (Vec<ResponseEvent>, Option<TokenUsage>) {
    let mut events = Vec::new();

    // Add Created event
    events.push(ResponseEvent::Created);

    // Get raw response body from sdk_http_response for storage
    let full_response_json = message
        .sdk_http_response
        .as_ref()
        .and_then(|r| r.body.clone())
        .and_then(|body| EncryptedContent::from_body_str(&body, PROVIDER_SDK_ANTHROPIC))
        .and_then(|ec| ec.to_json_string());

    // Collect text content for a single Message event
    let mut text_parts: Vec<String> = Vec::new();
    let mut has_reasoning = false;

    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                text_parts.push(text.clone());
            }

            ContentBlock::ToolUse { id, name, input } => {
                // Flush accumulated text first
                if !text_parts.is_empty() {
                    events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                        id: None,
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText {
                            text: text_parts.join(""),
                        }],
                    }));
                    text_parts.clear();
                }

                // Add function call event
                events.push(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    id: None,
                    call_id: id.clone(),
                    name: name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                }));
            }

            ContentBlock::Thinking { thinking, .. } => {
                // Flush accumulated text first
                if !text_parts.is_empty() {
                    events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                        id: None,
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText {
                            text: text_parts.join(""),
                        }],
                    }));
                    text_parts.clear();
                }

                // Add reasoning event with full response stored in encrypted_content
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: uuid::Uuid::new_v4().to_string(),
                    summary: vec![ReasoningItemReasoningSummary::SummaryText {
                        text: thinking.clone(),
                    }],
                    content: Some(vec![ReasoningItemContent::ReasoningText {
                        text: thinking.clone(),
                    }]),
                    encrypted_content: full_response_json.clone(),
                }));
                has_reasoning = true;
            }

            ContentBlock::RedactedThinking { .. } => {
                // Flush accumulated text first
                if !text_parts.is_empty() {
                    events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                        id: None,
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText {
                            text: text_parts.join(""),
                        }],
                    }));
                    text_parts.clear();
                }

                // Add redacted reasoning event with full response stored
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: uuid::Uuid::new_v4().to_string(),
                    summary: vec![],
                    content: None,
                    encrypted_content: full_response_json.clone(),
                }));
                has_reasoning = true;
            }
        }
    }

    // Flush any remaining text
    if !text_parts.is_empty() {
        events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: text_parts.join(""),
            }],
        }));
    }

    // If no reasoning block, create one to store full response for round-trip
    if !has_reasoning && full_response_json.is_some() {
        events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
            id: uuid::Uuid::new_v4().to_string(),
            summary: vec![],
            content: None,
            encrypted_content: full_response_json,
        }));
    }

    // Extract token usage
    let usage = extract_usage(&message.usage);

    // Add Completed event
    events.push(ResponseEvent::Completed {
        response_id: message.id.clone(),
        token_usage: Some(usage.clone()),
    });

    (events, Some(usage))
}

/// Extract token usage from Anthropic Usage.
fn extract_usage(usage: &anthropic_sdk::Usage) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.input_tokens as i64,
        output_tokens: usage.output_tokens as i64,
        cached_input_tokens: usage.cache_read_input_tokens.unwrap_or(0) as i64,
        total_tokens: (usage.input_tokens
            + usage.output_tokens
            + usage.cache_creation_input_tokens.unwrap_or(0)
            + usage.cache_read_input_tokens.unwrap_or(0)) as i64,
        reasoning_output_tokens: 0, // Anthropic includes thinking in output_tokens
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use anthropic_sdk::Role;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_prompt_to_messages_simple_user() {
        let prompt = Prompt {
            instructions: "You are helpful.".to_string(),
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Hello".to_string(),
                }],
            }],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let (messages, system) = prompt_to_messages(&prompt);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert!(system.is_some());
    }

    #[test]
    fn test_prompt_to_messages_with_function_call() {
        let prompt = Prompt {
            instructions: String::new(),
            input: vec![
                ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: "Run a command".to_string(),
                    }],
                },
                ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: "I'll run that for you.".to_string(),
                    }],
                },
                ResponseItem::FunctionCall {
                    id: None,
                    call_id: "call_123".to_string(),
                    name: "shell".to_string(),
                    arguments: r#"{"command": "ls"}"#.to_string(),
                },
                ResponseItem::FunctionCallOutput {
                    call_id: "call_123".to_string(),
                    output: FunctionCallOutputPayload {
                        content: "file1.txt\nfile2.txt".to_string(),
                        content_items: None,
                        success: Some(true),
                    },
                },
            ],
            tools: vec![],
            parallel_tool_calls: false,
            output_schema: None,
            previous_response_id: None,
        };

        let (messages, _) = prompt_to_messages(&prompt);

        // Should have: user, assistant (with text + tool_use), user (tool_result)
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[2].role, Role::User);

        // Check assistant message has both text and tool_use
        assert_eq!(messages[1].content.len(), 2);
    }

    #[test]
    fn test_tools_to_anthropic_openai_format() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get current weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }
            }
        })];

        let anthropic_tools = tools_to_anthropic(&tools);

        assert_eq!(anthropic_tools.len(), 1);
        assert_eq!(anthropic_tools[0].name, "get_weather");
        assert_eq!(
            anthropic_tools[0].description,
            Some("Get current weather".to_string())
        );
    }

    #[test]
    fn test_tools_to_anthropic_direct_format() {
        let tools = vec![serde_json::json!({
            "name": "search",
            "description": "Search the web",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }
        })];

        let anthropic_tools = tools_to_anthropic(&tools);

        assert_eq!(anthropic_tools.len(), 1);
        assert_eq!(anthropic_tools[0].name, "search");
    }

    #[test]
    fn test_parse_image_url_data_url() {
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
        let block = parse_image_url_to_block(data_url);

        match block {
            ContentBlockParam::Image { source, .. } => match source {
                anthropic_sdk::ImageSource::Base64 { media_type, .. } => {
                    assert_eq!(media_type, ImageMediaType::Png);
                }
                _ => panic!("expected Base64 source"),
            },
            _ => panic!("expected Image block"),
        }
    }

    #[test]
    fn test_parse_image_url_regular_url() {
        let url = "https://example.com/image.png";
        let block = parse_image_url_to_block(url);

        match block {
            ContentBlockParam::Image { source, .. } => match source {
                anthropic_sdk::ImageSource::Url { url: parsed_url } => {
                    assert_eq!(parsed_url, url);
                }
                _ => panic!("expected Url source"),
            },
            _ => panic!("expected Image block"),
        }
    }
}
