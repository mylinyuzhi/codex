use vercel_ai_provider::{
    AssistantContentPart, LanguageModelV4Message, LanguageModelV4Prompt, UserContentPart,
};

/// Convert a `LanguageModelV4Prompt` to a single text prompt for the legacy Completions API.
pub fn convert_to_completion_prompt(prompt: &LanguageModelV4Prompt) -> String {
    let mut parts = Vec::new();

    for msg in prompt {
        match msg {
            LanguageModelV4Message::System { content, .. } => {
                parts.push(content.clone());
            }
            LanguageModelV4Message::User { content, .. } => {
                for part in content {
                    if let UserContentPart::Text(tp) = part {
                        parts.push(tp.text.clone());
                    }
                }
            }
            LanguageModelV4Message::Assistant { content, .. } => {
                for part in content {
                    if let AssistantContentPart::Text(tp) = part {
                        parts.push(tp.text.clone());
                    }
                }
            }
            LanguageModelV4Message::Tool { .. } => {
                // Tool content not supported in completions
            }
        }
    }

    parts.join("\n\n")
}

#[cfg(test)]
#[path = "convert_to_completion_prompt.test.rs"]
mod tests;
