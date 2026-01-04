//! Conversion functions between codex-api and Volcengine Ark SDK types.

use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;
use volcengine_ark_sdk::ImageMediaType;
use volcengine_ark_sdk::InputContentBlock;
use volcengine_ark_sdk::InputMessage;
use volcengine_ark_sdk::OutputContentBlock;
use volcengine_ark_sdk::OutputItem;
use volcengine_ark_sdk::Response;
use volcengine_ark_sdk::Tool;
use volcengine_ark_sdk::ToolChoice;

use crate::common::Prompt;
use crate::common::ResponseEvent;

// ============================================================================
// Request conversion: Prompt -> Ark messages
// ============================================================================

/// Convert a codex-api Prompt to Ark InputMessages and optional system instructions.
///
/// This function handles the conversion of:
/// - User messages -> InputMessage with role="user"
/// - Assistant messages -> InputMessage with role="assistant"
/// - FunctionCall -> Appended as text to assistant message (Ark handles function calls at output level)
/// - FunctionCallOutput -> InputMessage with function_call_output content
/// - Reasoning -> Skipped (handled by ThinkingConfig)
pub fn prompt_to_messages(prompt: &Prompt) -> (Vec<InputMessage>, Option<String>) {
    let mut messages: Vec<InputMessage> = Vec::new();
    let mut current_assistant_content: Vec<InputContentBlock> = Vec::new();

    for item in &prompt.input {
        match item {
            ResponseItem::Message { role, content, .. } => {
                if role == "assistant" {
                    // Continue or start assistant message
                    current_assistant_content.extend(content.iter().map(content_item_to_block));
                } else {
                    // Flush any pending assistant message first
                    flush_assistant_message(&mut messages, &mut current_assistant_content);

                    // Add user message
                    let blocks: Vec<InputContentBlock> =
                        content.iter().map(content_item_to_block).collect();
                    if !blocks.is_empty() {
                        messages.push(InputMessage::user(blocks));
                    }
                }
            }

            ResponseItem::FunctionCall {
                name, arguments, ..
            } => {
                // For Ark, function calls from assistant are represented as text in the conversation
                // The actual function call happens in the response. We include it as context.
                let text = format!("[Called function: {} with arguments: {}]", name, arguments);
                current_assistant_content.push(InputContentBlock::text(text));
            }

            ResponseItem::FunctionCallOutput { call_id, output } => {
                // Flush assistant message first (tool result must follow assistant message)
                flush_assistant_message(&mut messages, &mut current_assistant_content);

                // Add function call output as user message
                let content = function_output_to_block(call_id, output);
                messages.push(InputMessage::user(vec![content]));
            }

            ResponseItem::Reasoning { .. } => {
                // Reasoning is handled by ThinkingConfig, skip in messages
            }

            // Skip types not applicable to Ark API
            _ => {}
        }
    }

    // Flush any remaining assistant content
    flush_assistant_message(&mut messages, &mut current_assistant_content);

    // Extract system prompt
    let system_prompt = if prompt.instructions.is_empty() {
        None
    } else {
        Some(prompt.instructions.clone())
    };

    (messages, system_prompt)
}

/// Flush the current assistant message content to the messages list.
fn flush_assistant_message(
    messages: &mut Vec<InputMessage>,
    current_content: &mut Vec<InputContentBlock>,
) {
    if !current_content.is_empty() {
        messages.push(InputMessage::assistant(std::mem::take(current_content)));
    }
}

/// Convert a ContentItem to an Ark InputContentBlock.
fn content_item_to_block(item: &ContentItem) -> InputContentBlock {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
            InputContentBlock::text(text)
        }
        ContentItem::InputImage { image_url } => parse_image_url_to_block(image_url),
    }
}

/// Parse a MIME type string to an Ark ImageMediaType.
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

/// Parse an image URL (data URL or regular URL) to an Ark InputContentBlock.
fn parse_image_url_to_block(image_url: &str) -> InputContentBlock {
    if let Some(data_url) = image_url.strip_prefix("data:") {
        if let Some((mime_and_encoding, data)) = data_url.split_once(',') {
            let media_type = parse_media_type(mime_and_encoding);
            return InputContentBlock::image_base64(data, media_type);
        }
    }
    InputContentBlock::image_url(image_url)
}

/// Convert FunctionCallOutput to an InputContentBlock.
fn function_output_to_block(
    call_id: &str,
    output: &FunctionCallOutputPayload,
) -> InputContentBlock {
    let is_error = if output.success == Some(false) {
        Some(true)
    } else {
        None
    };
    InputContentBlock::function_call_output(call_id, &output.content, is_error)
}

// ============================================================================
// Tool conversion: JSON -> Ark Tool
// ============================================================================

/// Convert JSON tool definitions to Ark Tool structs.
///
/// Supports both OpenAI-style format:
/// ```json
/// {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
/// ```
/// And direct function format:
/// ```json
/// {"name": "...", "description": "...", "parameters": {...}}
/// ```
pub fn tools_to_ark(tools: &[Value]) -> Vec<Tool> {
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

/// Convert a single tool JSON to an Ark Tool struct.
fn tool_json_to_struct(json: &Value) -> Option<Tool> {
    let name = json.get("name")?.as_str()?;
    let description = json
        .get("description")
        .and_then(|d| d.as_str())
        .map(String::from);
    let parameters = json
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

    Tool::function(name, description, parameters).ok()
}

/// Convert Ark ToolChoice enum from extra config.
pub fn parse_tool_choice(extra: &Option<Value>) -> Option<ToolChoice> {
    let tool_choice = extra.as_ref()?.get("tool_choice")?.as_str()?;
    match tool_choice {
        "auto" => Some(ToolChoice::Auto),
        "none" => Some(ToolChoice::None),
        "required" => Some(ToolChoice::Required),
        _ => None,
    }
}

// ============================================================================
// Response conversion: Ark Response -> ResponseEvents
// ============================================================================

/// Convert an Ark Response to codex-api ResponseEvents.
///
/// Returns a vector of events and optional token usage.
pub fn response_to_events(response: &Response) -> (Vec<ResponseEvent>, Option<TokenUsage>) {
    let mut events = Vec::new();

    // Add Created event
    events.push(ResponseEvent::Created);

    for item in &response.output {
        match item {
            OutputItem::Message { content, .. } => {
                // Collect text content
                let mut text_parts: Vec<String> = Vec::new();

                for block in content {
                    match block {
                        OutputContentBlock::Text { text } => {
                            text_parts.push(text.clone());
                        }
                        OutputContentBlock::Thinking { thinking, .. } => {
                            // Add reasoning event for thinking content
                            events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                                id: uuid::Uuid::new_v4().to_string(),
                                summary: vec![ReasoningItemReasoningSummary::SummaryText {
                                    text: thinking.clone(),
                                }],
                                content: Some(vec![ReasoningItemContent::ReasoningText {
                                    text: thinking.clone(),
                                }]),
                                encrypted_content: None,
                            }));
                        }
                        OutputContentBlock::FunctionCall {
                            id,
                            name,
                            arguments,
                        } => {
                            // Flush text first
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
                            // Note: Ark uses `id` as the call_id, and arguments is serde_json::Value
                            events.push(ResponseEvent::OutputItemDone(
                                ResponseItem::FunctionCall {
                                    id: None,
                                    call_id: id.clone(),
                                    name: name.clone(),
                                    arguments: serde_json::to_string(arguments).unwrap_or_default(),
                                },
                            ));
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
            }

            OutputItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    id: None,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }));
            }

            OutputItem::Reasoning {
                id,
                content,
                summary,
                ..
            } => {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    summary: summary
                        .as_ref()
                        .map(|summaries| {
                            summaries
                                .iter()
                                .map(|s| ReasoningItemReasoningSummary::SummaryText {
                                    text: s.text.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    content: Some(vec![ReasoningItemContent::ReasoningText {
                        text: content.clone(),
                    }]),
                    encrypted_content: None,
                }));
            }
        }
    }

    // Extract token usage
    let usage = extract_usage(&response.usage);

    // Add Completed event
    events.push(ResponseEvent::Completed {
        response_id: response.id.clone(),
        token_usage: Some(usage.clone()),
    });

    (events, Some(usage))
}

/// Extract token usage from Ark Usage.
fn extract_usage(usage: &volcengine_ark_sdk::Usage) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.input_tokens as i64,
        output_tokens: usage.output_tokens as i64,
        cached_input_tokens: usage.cached_tokens() as i64,
        total_tokens: usage.total_tokens as i64,
        reasoning_output_tokens: usage.reasoning_tokens() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(system.is_some());
        assert_eq!(system.unwrap(), "You are helpful.");
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

        // Should have: user, assistant (with text + function_call as text), user (function_output)
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_tools_to_ark_openai_format() {
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

        let ark_tools = tools_to_ark(&tools);

        assert_eq!(ark_tools.len(), 1);
        assert_eq!(ark_tools[0].function.name, "get_weather");
        assert_eq!(
            ark_tools[0].function.description,
            Some("Get current weather".to_string())
        );
    }

    #[test]
    fn test_tools_to_ark_direct_format() {
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

        let ark_tools = tools_to_ark(&tools);

        assert_eq!(ark_tools.len(), 1);
        assert_eq!(ark_tools[0].function.name, "search");
    }

    #[test]
    fn test_parse_image_url_data_url() {
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
        let block = parse_image_url_to_block(data_url);

        match block {
            InputContentBlock::Image { source, .. } => {
                // Verify it's base64 encoded
                assert!(matches!(
                    source,
                    volcengine_ark_sdk::ImageSource::Base64 { .. }
                ));
            }
            _ => panic!("expected Image block"),
        }
    }

    #[test]
    fn test_parse_image_url_regular_url() {
        let url = "https://example.com/image.png";
        let block = parse_image_url_to_block(url);

        match block {
            InputContentBlock::Image { source, .. } => match source {
                volcengine_ark_sdk::ImageSource::Url { url: parsed_url } => {
                    assert_eq!(parsed_url, url);
                }
                _ => panic!("expected Url source"),
            },
            _ => panic!("expected Image block"),
        }
    }
}
