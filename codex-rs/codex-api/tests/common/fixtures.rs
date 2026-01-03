//! Test fixtures and data generators for integration tests.
//!
//! This module provides reusable test data including prompts, tools,
//! and image content for testing various adapter features.

#![allow(dead_code)] // Utility functions may not all be used yet

use codex_api::Prompt;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use serde_json::json;

/// Create a simple text prompt for testing.
pub fn text_prompt(content: &str) -> Prompt {
    Prompt {
        instructions: "You are a helpful assistant. Be concise.".to_string(),
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: content.to_string(),
            }],
        }],
        tools: vec![],
        parallel_tool_calls: false,
        output_schema: None,
        previous_response_id: None,
    }
}

/// Create a prompt with tool definitions for testing tool calling.
pub fn tool_prompt(content: &str, tools: Vec<Value>) -> Prompt {
    Prompt {
        instructions: "You are a helpful assistant. Use the provided tools when appropriate."
            .to_string(),
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: content.to_string(),
            }],
        }],
        tools,
        parallel_tool_calls: true,
        output_schema: None,
        previous_response_id: None,
    }
}

/// Create a prompt with an image for testing multi-modal capabilities.
pub fn image_prompt(content: &str, image_data_url: &str) -> Prompt {
    Prompt {
        instructions: "You are a helpful assistant that can analyze images.".to_string(),
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![
                ContentItem::InputImage {
                    image_url: image_data_url.to_string(),
                },
                ContentItem::InputText {
                    text: content.to_string(),
                },
            ],
        }],
        tools: vec![],
        parallel_tool_calls: false,
        output_schema: None,
        previous_response_id: None,
    }
}

/// Create a prompt with previous_response_id for conversation continuity testing.
pub fn continuation_prompt(content: &str, previous_response_id: String) -> Prompt {
    Prompt {
        instructions: "You are a helpful assistant.".to_string(),
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: content.to_string(),
            }],
        }],
        tools: vec![],
        parallel_tool_calls: false,
        output_schema: None,
        previous_response_id: Some(previous_response_id),
    }
}

/// Create a simple weather tool definition for testing.
pub fn weather_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the current weather for a city",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "The city name"
                    }
                },
                "required": ["city"]
            }
        }
    })
}

/// Create a calculator tool definition for testing.
pub fn calculator_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "calculate",
            "description": "Perform a mathematical calculation",
            "parameters": {
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "The mathematical expression to evaluate"
                    }
                },
                "required": ["expression"]
            }
        }
    })
}

/// A small 10x10 red square PNG image encoded as base64 data URL.
/// Used for testing image handling capabilities.
pub const TEST_RED_SQUARE_BASE64: &str = "data:image/png;base64,\
iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR4AWNgGAWjYBSMglEwCkgHAA+IAAT6kbF5AAAAAElFTkSuQmCC";

/// A small 10x10 blue square PNG image encoded as base64 data URL.
pub const TEST_BLUE_SQUARE_BASE64: &str = "data:image/png;base64,\
iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR4AWP4//8/w0AmGAWjgHIAABZQAQVmGY6GAAAAAElFTkSuQmCC";

/// Extract text content from a GenerateResult.
pub fn extract_text(result: &codex_api::GenerateResult) -> String {
    result
        .events
        .iter()
        .filter_map(|event| {
            if let codex_api::ResponseEvent::OutputItemDone(ResponseItem::Message {
                content, ..
            }) = event
            {
                Some(
                    content
                        .iter()
                        .filter_map(|c| {
                            if let ContentItem::OutputText { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                )
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Check if result contains a function call with the given name.
pub fn has_function_call(result: &codex_api::GenerateResult, name: &str) -> bool {
    result.events.iter().any(|event| {
        matches!(
            event,
            codex_api::ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { name: n, .. })
            if n == name
        )
    })
}

/// Check if result contains reasoning/thinking content.
pub fn has_reasoning(result: &codex_api::GenerateResult) -> bool {
    result.events.iter().any(|event| {
        matches!(
            event,
            codex_api::ResponseEvent::OutputItemDone(ResponseItem::Reasoning { .. })
        )
    })
}

/// Extracted function call information.
#[derive(Debug, Clone)]
pub struct FunctionCallInfo {
    pub name: String,
    pub arguments: String,
    pub call_id: String,
}

/// Extract all function calls from a GenerateResult.
pub fn extract_function_calls(result: &codex_api::GenerateResult) -> Vec<FunctionCallInfo> {
    result
        .events
        .iter()
        .filter_map(|event| {
            if let codex_api::ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            }) = event
            {
                Some(FunctionCallInfo {
                    name: name.clone(),
                    arguments: arguments.clone(),
                    call_id: call_id.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Create a multi-turn conversation prompt.
///
/// This builds a prompt with full conversation history for testing
/// multi-turn conversation capabilities.
pub fn multi_turn_prompt(
    history: Vec<ResponseItem>,
    user_message: &str,
    tools: Vec<Value>,
) -> Prompt {
    let mut input = history;
    input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: user_message.to_string(),
        }],
    });

    Prompt {
        instructions: "You are a helpful assistant.".to_string(),
        input,
        tools,
        parallel_tool_calls: true,
        output_schema: None,
        previous_response_id: None,
    }
}

/// Create a prompt with tool call and its output for the second turn.
///
/// This simulates: User asks → LLM returns tool call → User provides tool output → LLM responds
pub fn tool_output_prompt(
    original_question: &str,
    function_call: &FunctionCallInfo,
    tool_output: &str,
    tools: Vec<Value>,
) -> Prompt {
    use codex_protocol::models::FunctionCallOutputPayload;

    Prompt {
        instructions:
            "You are a helpful assistant. Use tool results to answer the user's question."
                .to_string(),
        input: vec![
            // Original user question
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: original_question.to_string(),
                }],
            },
            // Assistant's function call
            ResponseItem::FunctionCall {
                id: None,
                name: function_call.name.clone(),
                arguments: function_call.arguments.clone(),
                call_id: function_call.call_id.clone(),
            },
            // Tool output
            ResponseItem::FunctionCallOutput {
                call_id: function_call.call_id.clone(),
                output: FunctionCallOutputPayload {
                    content: tool_output.to_string(),
                    content_items: None,
                    success: Some(true),
                },
            },
        ],
        tools,
        parallel_tool_calls: true,
        output_schema: None,
        previous_response_id: None,
    }
}

/// Create assistant message ResponseItem from text.
pub fn assistant_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
    }
}

/// Create user message ResponseItem from text.
pub fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
    }
}
