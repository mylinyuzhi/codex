//! Attachment configuration.
//!
//! Defines settings for response attachments.

use serde::Deserialize;
use serde::Serialize;

/// Attachment configuration.
///
/// Controls which attachments are included in responses.
///
/// # Environment Variables
///
/// - `COCODE_DISABLE_ATTACHMENTS`: Disable all attachments
/// - `COCODE_ENABLE_TOKEN_USAGE_ATTACHMENT`: Enable token usage attachment
///
/// # Example
///
/// ```json
/// {
///   "attachment": {
///     "disable_attachments": false,
///     "enable_token_usage_attachment": true
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct AttachmentConfig {
    /// Disable all attachments.
    #[serde(default)]
    pub disable_attachments: bool,

    /// Enable token usage attachment in responses.
    #[serde(default)]
    pub enable_token_usage_attachment: bool,
}

impl AttachmentConfig {
    /// Check if attachments are enabled.
    pub fn are_attachments_enabled(&self) -> bool {
        !self.disable_attachments
    }

    /// Check if token usage attachment should be included.
    pub fn should_include_token_usage(&self) -> bool {
        !self.disable_attachments && self.enable_token_usage_attachment
    }
}

#[cfg(test)]
#[path = "attachment_config.test.rs"]
mod tests;
