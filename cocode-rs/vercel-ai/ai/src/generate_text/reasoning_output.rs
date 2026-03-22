//! Reasoning output type.
//!
//! This module provides the `ReasoningOutput` type which represents
//! structured reasoning content from model responses, including
//! the text, optional signature, and provider metadata.

use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::ProviderMetadata;

/// Structured reasoning output from a model response.
///
/// This type represents a single reasoning block, matching the TS SDK's
/// reasoning output with `text`, `signature`, and `providerMetadata`.
#[derive(Debug, Clone)]
pub struct ReasoningOutput {
    /// The reasoning text content.
    pub text: String,
    /// Optional signature for the reasoning (e.g., for verification).
    pub signature: Option<String>,
    /// Provider-specific metadata for this reasoning block.
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ReasoningOutput {
    /// Create a new reasoning output with just text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            signature: None,
            provider_metadata: None,
        }
    }

    /// Set the signature.
    pub fn with_signature(mut self, signature: impl Into<String>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    /// Set the provider metadata.
    pub fn with_provider_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// Extract `ReasoningOutput` items from content parts.
///
/// Note: This function is also mirrored in `content_utils` for centralized access.
/// This version is kept here for the `reasoning_output` module's self-contained API.
#[allow(dead_code)]
pub fn extract_reasoning_outputs(content: &[AssistantContentPart]) -> Vec<ReasoningOutput> {
    content
        .iter()
        .filter_map(|part| match part {
            AssistantContentPart::Reasoning(r) => {
                let mut output = ReasoningOutput::new(&r.text);
                if let Some(ref pm) = r.provider_metadata {
                    output.provider_metadata = Some(pm.clone());
                }
                Some(output)
            }
            _ => None,
        })
        .collect()
}

/// Get the combined reasoning text from reasoning outputs.
pub fn reasoning_text(outputs: &[ReasoningOutput]) -> String {
    outputs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "reasoning_output.test.rs"]
mod tests;
