//! Extract text content from assistant content parts.
//!
//! This module provides utilities for extracting text from model responses.

use vercel_ai_provider::AssistantContentPart;

/// Extract all text content from a slice of assistant content parts.
///
/// This function concatenates all text from `Text` content parts,
/// ignoring other content types like reasoning, tool calls, etc.
///
/// # Arguments
///
/// * `content` - A slice of assistant content parts.
///
/// # Returns
///
/// A `String` containing all concatenated text content.
///
/// # Example
///
/// ```ignore
/// use vercel_ai_provider::AssistantContentPart;
/// use vercel_ai::generate_text::extract_text_content;
///
/// let content = vec![
///     AssistantContentPart::text("Hello, "),
///     AssistantContentPart::text("world!"),
/// ];
///
/// let text = extract_text_content(&content);
/// assert_eq!(text, "Hello, world!");
/// ```
pub fn extract_text_content(content: &[AssistantContentPart]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            AssistantContentPart::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Extract text content with metadata from assistant content parts.
///
/// This function returns both the concatenated text and information about
/// other content types present.
///
/// # Arguments
///
/// * `content` - A slice of assistant content parts.
///
/// # Returns
///
/// A tuple containing:
/// * The concatenated text content
/// * Whether any reasoning was present
/// * Whether any tool calls were present
pub fn extract_text_content_with_metadata(
    content: &[AssistantContentPart],
) -> (String, bool, bool) {
    let mut text = String::new();
    let mut has_reasoning = false;
    let mut has_tool_calls = false;

    for part in content {
        match part {
            AssistantContentPart::Text(t) => {
                text.push_str(&t.text);
            }
            AssistantContentPart::Reasoning(_) => {
                has_reasoning = true;
            }
            AssistantContentPart::ToolCall(_) => {
                has_tool_calls = true;
            }
            _ => {}
        }
    }

    (text, has_reasoning, has_tool_calls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vercel_ai_provider::ReasoningPart;
    use vercel_ai_provider::TextPart;

    #[test]
    fn test_extract_text_content_empty() {
        let content: Vec<AssistantContentPart> = vec![];
        assert_eq!(extract_text_content(&content), "");
    }

    #[test]
    fn test_extract_text_content_single() {
        let content = vec![AssistantContentPart::Text(TextPart {
            text: "Hello, world!".to_string(),
            provider_metadata: None,
        })];
        assert_eq!(extract_text_content(&content), "Hello, world!");
    }

    #[test]
    fn test_extract_text_content_multiple() {
        let content = vec![
            AssistantContentPart::Text(TextPart {
                text: "Hello, ".to_string(),
                provider_metadata: None,
            }),
            AssistantContentPart::Text(TextPart {
                text: "world!".to_string(),
                provider_metadata: None,
            }),
        ];
        assert_eq!(extract_text_content(&content), "Hello, world!");
    }

    #[test]
    fn test_extract_text_content_mixed() {
        let content = vec![
            AssistantContentPart::Text(TextPart {
                text: "Some text".to_string(),
                provider_metadata: None,
            }),
            AssistantContentPart::Reasoning(ReasoningPart {
                text: "Thinking...".to_string(),
                provider_metadata: None,
            }),
            AssistantContentPart::Text(TextPart {
                text: " more text".to_string(),
                provider_metadata: None,
            }),
        ];
        assert_eq!(extract_text_content(&content), "Some text more text");
    }

    #[test]
    fn test_extract_text_content_with_metadata() {
        let content = vec![
            AssistantContentPart::Text(TextPart {
                text: "Hello".to_string(),
                provider_metadata: None,
            }),
            AssistantContentPart::Reasoning(ReasoningPart {
                text: "thinking".to_string(),
                provider_metadata: None,
            }),
            AssistantContentPart::ToolCall(vercel_ai_provider::ToolCallPart {
                tool_call_id: "call_1".to_string(),
                tool_name: "test_tool".to_string(),
                input: json!({}),
                provider_executed: None,
                provider_metadata: None,
            }),
        ];

        let (text, has_reasoning, has_tool_calls) = extract_text_content_with_metadata(&content);
        assert_eq!(text, "Hello");
        assert!(has_reasoning);
        assert!(has_tool_calls);
    }
}
