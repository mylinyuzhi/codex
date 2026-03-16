use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::UserContentPart;

/// Result of converting a prompt to completion format.
pub struct CompletionPromptResult {
    /// The formatted prompt text.
    pub prompt: String,
    /// Stop sequences to use (e.g., `["\nuser:"]`).
    pub stop_sequences: Vec<String>,
}

/// Convert a `LanguageModelV4Prompt` to a single text prompt for the legacy Completions API.
///
/// Produces structured format matching the TS implementation:
/// ```text
/// {system}\n\n
/// user:\n{content}\n\n
/// assistant:\n{content}\n\n
/// assistant:\n
/// ```
///
/// Returns the prompt text and stop sequences (`["\nuser:"]`).
pub fn convert_to_completion_prompt(prompt: &LanguageModelV4Prompt) -> CompletionPromptResult {
    let user = "user";
    let assistant = "assistant";

    let mut text = String::new();
    let mut iter = prompt.iter().peekable();

    // If first message is a system message, add it without prefix
    if let Some(LanguageModelV4Message::System { content, .. }) = iter.peek() {
        text.push_str(content);
        text.push_str("\n\n");
        iter.next();
    }

    for msg in iter {
        match msg {
            LanguageModelV4Message::System { .. } => {
                // TS throws InvalidPromptError for system messages after the first.
                // In Rust, we skip them gracefully.
            }
            LanguageModelV4Message::User { content, .. } => {
                let user_message: String = content
                    .iter()
                    .filter_map(|part| {
                        if let UserContentPart::Text(tp) = part {
                            Some(tp.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                text.push_str(&format!("{user}:\n{user_message}\n\n"));
            }
            LanguageModelV4Message::Assistant { content, .. } => {
                let assistant_message: String = content
                    .iter()
                    .filter_map(|part| {
                        if let AssistantContentPart::Text(tp) = part {
                            Some(tp.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                text.push_str(&format!("{assistant}:\n{assistant_message}\n\n"));
            }
            LanguageModelV4Message::Tool { .. } => {
                // Tool content not supported in completions (TS throws UnsupportedFunctionalityError)
            }
        }
    }

    // Add assistant prefix for the model to continue from
    text.push_str(&format!("{assistant}:\n"));

    CompletionPromptResult {
        prompt: text,
        stop_sequences: vec![format!("\n{user}:")],
    }
}

#[cfg(test)]
#[path = "convert_to_completion_prompt.test.rs"]
mod tests;
