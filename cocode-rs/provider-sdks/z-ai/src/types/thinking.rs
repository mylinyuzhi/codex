//! Extended thinking configuration for Z.AI SDK.

use serde::Deserialize;
use serde::Serialize;

/// Extended thinking configuration.
///
/// Controls whether the model outputs reasoning steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[derive(Default)]
pub enum ThinkingConfig {
    /// Enable extended thinking.
    Enabled {
        /// Optional budget tokens for thinking.
        #[serde(skip_serializing_if = "Option::is_none")]
        budget_tokens: Option<i32>,
    },
    /// Disable extended thinking.
    #[default]
    Disabled,
}

impl ThinkingConfig {
    /// Create an enabled thinking config without budget.
    pub fn enabled() -> Self {
        Self::Enabled {
            budget_tokens: None,
        }
    }

    /// Create an enabled thinking config with budget.
    pub fn enabled_with_budget(budget_tokens: i32) -> Self {
        Self::Enabled {
            budget_tokens: Some(budget_tokens),
        }
    }

    /// Create a disabled thinking config.
    pub fn disabled() -> Self {
        Self::Disabled
    }
}
