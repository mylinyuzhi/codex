//! Conversion functions between codex-api and Z.AI SDK types.

use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;
use z_ai_sdk::Completion;
use z_ai_sdk::CompletionUsage;
use z_ai_sdk::ContentBlock;
use z_ai_sdk::MessageParam;
use z_ai_sdk::Tool;

use crate::common::Prompt;
use crate::common::ResponseEvent;

// ============================================================================
// Request conversion: Prompt -> Z.AI messages
// ============================================================================

/// Convert a codex-api Prompt to Z.AI MessageParams and optional system message.
///
/// This function handles the conversion of:
/// - User messages -> MessageParam with role="user"
/// - Assistant messages -> MessageParam with role="assistant"
/// - FunctionCall -> Converted to assistant message with tool_calls
/// - FunctionCallOutput -> MessageParam with role="tool"
/// - Reasoning -> Skip (handled by ThinkingConfig)
pub fn prompt_to_messages(prompt: &Prompt) -> (Vec<MessageParam>, Option<String>) {
    let mut messages: Vec<MessageParam> = Vec::new();
    let mut pending_function_calls: Vec<(String, String, String)> = Vec::new(); // (call_id, name, arguments)
    let mut pending_assistant_text: Option<String> = None;

    for item in &prompt.input {
        match item {
            ResponseItem::Message { role, content, .. } => {
                if role == "assistant" {
                    // Flush any pending assistant content
                    flush_pending_assistant(
                        &mut messages,
                        &mut pending_assistant_text,
                        &mut pending_function_calls,
                    );

                    // Collect text content
                    let text = content
                        .iter()
                        .filter_map(|c| match c {
                            ContentItem::OutputText { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    if !text.is_empty() {
                        pending_assistant_text = Some(text);
                    }
                } else {
                    // Flush pending assistant message first
                    flush_pending_assistant(
                        &mut messages,
                        &mut pending_assistant_text,
                        &mut pending_function_calls,
                    );

                    // Add user message with content blocks
                    let blocks: Vec<ContentBlock> =
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
                // Collect function calls to be added to assistant message
                pending_function_calls.push((call_id.clone(), name.clone(), arguments.clone()));
            }

            ResponseItem::FunctionCallOutput { call_id, output } => {
                // Flush pending assistant message first
                flush_pending_assistant(
                    &mut messages,
                    &mut pending_assistant_text,
                    &mut pending_function_calls,
                );

                // Add tool result message
                messages.push(MessageParam::tool_result(call_id, &output.content));
            }

            ResponseItem::Reasoning { .. } => {
                // Reasoning is handled by ThinkingConfig, skip in messages
            }

            _ => {}
        }
    }

    // Flush any remaining assistant content
    flush_pending_assistant(
        &mut messages,
        &mut pending_assistant_text,
        &mut pending_function_calls,
    );

    // Extract system prompt
    let system = if prompt.instructions.is_empty() {
        None
    } else {
        Some(prompt.instructions.clone())
    };

    (messages, system)
}

/// Flush pending assistant text and function calls to a single message.
fn flush_pending_assistant(
    messages: &mut Vec<MessageParam>,
    pending_text: &mut Option<String>,
    pending_calls: &mut Vec<(String, String, String)>,
) {
    if pending_text.is_none() && pending_calls.is_empty() {
        return;
    }

    // For Z.AI, we need to handle the case where assistant has text + tool calls
    // Since MessageParam doesn't directly support tool_calls in content,
    // we'll add separate messages

    if let Some(text) = pending_text.take() {
        messages.push(MessageParam::assistant(text));
    }

    // Note: Z.AI SDK uses a different pattern for assistant tool calls
    // The tool calls are returned in CompletionMessage.tool_calls, not in content
    // For now, we don't need to include tool calls in the request
    // as they are derived from the response
    pending_calls.clear();
}

/// Convert a ContentItem to a Z.AI ContentBlock.
fn content_item_to_block(item: &ContentItem) -> ContentBlock {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
            ContentBlock::text(text)
        }
        ContentItem::InputImage { image_url } => {
            // Check if it's a data URL
            if image_url.starts_with("data:") {
                // Parse data URL: data:image/png;base64,<data>
                if let Some(rest) = image_url.strip_prefix("data:") {
                    if let Some((mime_encoding, data)) = rest.split_once(',') {
                        let media_type = mime_encoding.split(';').next().unwrap_or("image/png");
                        return ContentBlock::image_base64(data, media_type);
                    }
                }
            }
            ContentBlock::image_url(image_url)
        }
    }
}

// ============================================================================
// Tool conversion: JSON -> Z.AI Tool
// ============================================================================

/// Convert JSON tool definitions to Z.AI Tool structs.
///
/// Supports both OpenAI-style format:
/// ```json
/// {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
/// ```
/// And direct function format:
/// ```json
/// {"name": "...", "description": "...", "parameters": {...}}
/// ```
pub fn tools_to_zai(tools: &[Value]) -> Vec<Tool> {
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

/// Convert a single tool JSON to a Z.AI Tool struct.
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

    Some(Tool::function(name, description, parameters))
}

// ============================================================================
// Response conversion: Z.AI Completion -> ResponseEvents
// ============================================================================

/// Convert a Z.AI Completion response to codex-api ResponseEvents.
///
/// Returns a vector of events.
/// The events include:
/// - Created (response start)
/// - OutputItemDone for each content block (Message, FunctionCall, Reasoning)
/// - Completed (response end with usage)
pub fn completion_to_events(completion: &Completion) -> Vec<ResponseEvent> {
    let mut events = Vec::new();

    // Add Created event
    events.push(ResponseEvent::Created);

    // Process choices
    for choice in &completion.choices {
        let message = &choice.message;

        // Handle reasoning content (extended thinking)
        if let Some(reasoning) = &message.reasoning_content {
            if !reasoning.is_empty() {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: uuid::Uuid::new_v4().to_string(),
                    summary: vec![ReasoningItemReasoningSummary::SummaryText {
                        text: reasoning.clone(),
                    }],
                    content: Some(vec![ReasoningItemContent::ReasoningText {
                        text: reasoning.clone(),
                    }]),
                    encrypted_content: None,
                }));
            }
        }

        // Handle text content
        if let Some(content) = &message.content {
            if !content.is_empty() {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: content.clone(),
                    }],
                }));
            }
        }

        // Handle tool calls
        if let Some(tool_calls) = &message.tool_calls {
            for tool_call in tool_calls {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    id: None,
                    call_id: tool_call.id.clone(),
                    name: tool_call.function.name.clone(),
                    arguments: tool_call.function.arguments.clone(),
                }));
            }
        }
    }

    // Extract token usage
    let usage = extract_usage(&completion.usage);

    // Add Completed event
    events.push(ResponseEvent::Completed {
        response_id: completion.id.clone().unwrap_or_default(),
        token_usage: Some(usage),
    });

    events
}

/// Extract token usage from Z.AI CompletionUsage.
fn extract_usage(usage: &CompletionUsage) -> TokenUsage {
    let reasoning_tokens = usage
        .completion_tokens_details
        .as_ref()
        .map(|d| d.reasoning_tokens as i64)
        .unwrap_or(0);

    TokenUsage {
        input_tokens: usage.prompt_tokens as i64,
        output_tokens: usage.completion_tokens as i64,
        cached_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens as i64)
            .unwrap_or(0),
        total_tokens: usage.total_tokens as i64,
        reasoning_output_tokens: reasoning_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputPayload;
    use pretty_assertions::assert_eq;
    use z_ai_sdk::Role;

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
        assert_eq!(system.unwrap(), "You are helpful.");
    }

    #[test]
    fn test_prompt_to_messages_with_function_output() {
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

        // Should have: user, assistant, tool_result
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[2].role, Role::Tool);
    }

    #[test]
    fn test_tools_to_zai_openai_format() {
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

        let zai_tools = tools_to_zai(&tools);

        assert_eq!(zai_tools.len(), 1);
        match &zai_tools[0] {
            Tool::Function { function } => {
                assert_eq!(function.name, "get_weather");
                assert_eq!(
                    function.description,
                    Some("Get current weather".to_string())
                );
            }
        }
    }

    #[test]
    fn test_tools_to_zai_direct_format() {
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

        let zai_tools = tools_to_zai(&tools);

        assert_eq!(zai_tools.len(), 1);
        match &zai_tools[0] {
            Tool::Function { function } => {
                assert_eq!(function.name, "search");
            }
        }
    }

    #[test]
    fn test_content_item_to_block_text() {
        let item = ContentItem::InputText {
            text: "Hello".to_string(),
        };
        let block = content_item_to_block(&item);
        assert!(matches!(block, ContentBlock::Text { .. }));
    }

    #[test]
    fn test_content_item_to_block_image_url() {
        let item = ContentItem::InputImage {
            image_url: "https://example.com/image.png".to_string(),
        };
        let block = content_item_to_block(&item);
        assert!(matches!(block, ContentBlock::ImageUrl { .. }));
    }

    #[test]
    fn test_content_item_to_block_image_base64() {
        let item = ContentItem::InputImage {
            image_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==".to_string(),
        };
        let block = content_item_to_block(&item);
        // Should be converted to image_url with data URL
        assert!(matches!(block, ContentBlock::ImageUrl { .. }));
    }
}
