//! Standardize prompt format.
//!
//! This module provides functions for converting various prompt formats
//! into a standardized format that can be processed uniformly.

use vercel_ai_provider::LanguageModelV4Message;
use vercel_ai_provider::LanguageModelV4Prompt;

use crate::error::AIError;

/// A standardized prompt with system messages and conversation messages.
#[derive(Debug, Clone, Default)]
pub struct StandardizedPrompt {
    /// System messages (prepended to the conversation).
    pub system: Option<Vec<LanguageModelV4Message>>,
    /// The conversation messages.
    pub messages: LanguageModelV4Prompt,
}

impl StandardizedPrompt {
    /// Create a new empty standardized prompt.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add system messages.
    pub fn with_system(mut self, system: Vec<LanguageModelV4Message>) -> Self {
        self.system = Some(system);
        self
    }

    /// Add a single system message.
    pub fn with_system_text(mut self, text: impl Into<String>) -> Self {
        let system = vec![LanguageModelV4Message::system(text)];
        self.system = Some(system);
        self
    }

    /// Set the messages.
    pub fn with_messages(mut self, messages: LanguageModelV4Prompt) -> Self {
        self.messages = messages;
        self
    }
}

/// Standardize a prompt from text.
///
/// Converts a simple text prompt into a standardized format with a user message.
pub fn standardize_text_prompt(text: impl Into<String>) -> StandardizedPrompt {
    StandardizedPrompt::new().with_messages(vec![LanguageModelV4Message::user_text(text)])
}

/// Standardize a prompt from messages.
///
/// Validates and standardizes a list of messages.
pub fn standardize_messages_prompt(
    messages: LanguageModelV4Prompt,
) -> Result<StandardizedPrompt, AIError> {
    if messages.is_empty() {
        return Err(AIError::InvalidArgument(
            "messages must not be empty".to_string(),
        ));
    }

    Ok(StandardizedPrompt::new().with_messages(messages))
}

/// Standardize a prompt with optional system message.
///
/// This is the main entry point for standardizing prompts.
pub fn standardize_prompt(
    prompt: Option<String>,
    messages: Option<LanguageModelV4Prompt>,
    system: Option<String>,
) -> Result<StandardizedPrompt, AIError> {
    // Validate that either prompt or messages is provided
    match (prompt, messages) {
        (Some(p), None) => {
            let standardized = standardize_text_prompt(p);
            Ok(if let Some(sys) = system {
                standardized.with_system_text(sys)
            } else {
                standardized
            })
        }
        (None, Some(msgs)) => {
            let standardized = standardize_messages_prompt(msgs)?;
            Ok(if let Some(sys) = system {
                standardized.with_system_text(sys)
            } else {
                standardized
            })
        }
        (Some(_), Some(_)) => Err(AIError::InvalidArgument(
            "prompt and messages cannot be defined at the same time".to_string(),
        )),
        (None, None) => Err(AIError::InvalidArgument(
            "prompt or messages must be defined".to_string(),
        )),
    }
}

#[cfg(test)]
#[path = "standardize.test.rs"]
mod tests;
