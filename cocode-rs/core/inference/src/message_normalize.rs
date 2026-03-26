//! Per-provider message normalization.
//!
//! Applied as the final step in `RequestBuilder::build()` to ensure prompt
//! messages conform to provider-specific requirements.
//!
//! - Anthropic: rejects empty content parts/messages, requires alphanumeric tool IDs.
//! - Other providers: no normalization needed (currently).

use crate::AssistantContentPart;
use crate::LanguageModelMessage;
use crate::LanguageModelPrompt;
use crate::ToolContentPart;
use cocode_protocol::ProviderApi;

/// Normalize prompt messages for the target provider.
///
/// Called as the final step before sending the request. Modifies messages
/// in-place to avoid provider-specific rejections.
pub fn normalize_prompt(prompt: &mut LanguageModelPrompt, provider: ProviderApi) {
    if provider == ProviderApi::Anthropic {
        remove_empty_content_parts(prompt);
        remove_empty_messages(prompt);
        sanitize_tool_call_ids(prompt);
    }
}

/// Remove Text and Reasoning parts with empty text from assistant messages.
fn remove_empty_content_parts(prompt: &mut LanguageModelPrompt) {
    for msg in prompt.iter_mut() {
        if let LanguageModelMessage::Assistant { content, .. } = msg {
            content.retain(|part| match part {
                AssistantContentPart::Text(tp) => !tp.text.is_empty(),
                AssistantContentPart::Reasoning(rp) => !rp.text.is_empty(),
                _ => true,
            });
        }
    }
}

/// Remove messages with empty content arrays.
fn remove_empty_messages(prompt: &mut LanguageModelPrompt) {
    prompt.retain(|msg| match msg {
        LanguageModelMessage::Assistant { content, .. } => !content.is_empty(),
        _ => true,
    });
}

/// Replace non-alphanumeric/underscore/hyphen chars in tool_call_id fields with `_`.
///
/// Anthropic requires tool call IDs to match `[a-zA-Z0-9_-]+`.
fn sanitize_tool_call_ids(prompt: &mut LanguageModelPrompt) {
    for msg in prompt.iter_mut() {
        match msg {
            LanguageModelMessage::Assistant { content, .. } => {
                for part in content.iter_mut() {
                    if let AssistantContentPart::ToolCall(tc) = part {
                        tc.tool_call_id = sanitize_id(&tc.tool_call_id);
                    }
                }
            }
            LanguageModelMessage::Tool { content, .. } => {
                for part in content.iter_mut() {
                    if let ToolContentPart::ToolResult(result) = part {
                        result.tool_call_id = sanitize_id(&result.tool_call_id);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Replace non-`[a-zA-Z0-9_-]` characters with `_`.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "message_normalize.test.rs"]
mod tests;
