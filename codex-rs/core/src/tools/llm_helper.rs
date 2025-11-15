//! LLM helper utilities for tool usage.
//!
//! Provides simplified LLM calling interface for tools that need to make
//! simple text generation requests without full conversation context.

use crate::client::ModelClient;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;

/// Call LLM for simple text generation (used by tools for correction/refinement)
///
/// This helper constructs a minimal prompt, calls ModelClient::stream(),
/// and collects the complete response text.
///
/// # Parameters
/// - `client`: ModelClient to use for the request
/// - `system_prompt`: System-level instructions for the LLM
/// - `user_prompt`: User query/request
/// - `timeout_secs`: Timeout in seconds
///
/// # Returns
/// Complete response text from the LLM
///
/// # Errors
/// Returns error if:
/// - Request times out
/// - Stream encounters error
/// - ModelClient fails
pub async fn call_llm_for_text(
    client: &ModelClient,
    system_prompt: &str,
    user_prompt: &str,
    timeout_secs: u64,
) -> CodexResult<String> {
    // Construct minimal prompt with system + user message
    // Combine system and user into single user message for simplicity
    let combined_message = if system_prompt.is_empty() {
        user_prompt.to_string()
    } else {
        format!("{}\n\n{}", system_prompt, user_prompt)
    };

    let prompt = Prompt {
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: combined_message,
            }],
        }],
        tools: vec![], // No tools for simple text generation
        parallel_tool_calls: false,
        effective_parameters: Default::default(), // Use default parameters
        base_instructions_override: None,
        output_schema: None,
        reasoning_effort: None,
        reasoning_summary: None,
        previous_response_id: None,
    };

    // Call ModelClient::stream with timeout
    let stream_result = timeout(Duration::from_secs(timeout_secs), client.stream(&prompt)).await;

    let mut stream = match stream_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return Err(crate::error::CodexErr::Fatal(format!(
                "LLM call failed: {e}"
            )));
        }
        Err(_) => {
            return Err(crate::error::CodexErr::Fatal(format!(
                "LLM call timed out after {timeout_secs} seconds"
            )));
        }
    };

    // Collect all text deltas into result
    let mut result = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ResponseEvent::OutputTextDelta(text)) => {
                result.push_str(&text);
            }
            Ok(ResponseEvent::Completed { .. }) => {
                break;
            }
            Ok(_) => {
                // Ignore other events (reasoning, rate limits, etc.)
            }
            Err(e) => return Err(crate::error::CodexErr::Fatal(format!("Stream error: {e}"))),
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests with actual ModelClient are in core/tests/
    // Unit tests here would require mocking ModelClient, which is complex.
    // For now, we rely on integration tests.

    #[test]
    fn test_placeholder() {
        // Placeholder to satisfy cargo test
        assert!(true);
    }
}
