use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::str::FromStr;

/// Unified thinking configuration for all providers.
///
/// Replaces both TS EffortLevel and ThinkingConfig:
///   TS EffortLevel ('low'|'medium'|'high'|'max') → ThinkingLevel::low()/medium()/high()/xhigh()
///   TS ThinkingConfig { type: 'enabled', N }     → ThinkingLevel { effort: Medium, budget: Some(N) }
///   TS ThinkingConfig { type: 'disabled' }       → ThinkingLevel::none()
///
/// Only 2 typed fields (effort + budget_tokens) are universal across providers.
/// All provider-specific thinking params go through `options` (data-driven passthrough).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingLevel {
    /// Reasoning effort level — universal across all providers.
    pub effort: ReasoningEffort,

    /// Token budget — universal for budget-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<i32>,

    /// Provider-specific thinking extensions — data-driven passthrough.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub options: HashMap<String, serde_json::Value>,
}

impl ThinkingLevel {
    pub fn none() -> Self {
        Self {
            effort: ReasoningEffort::None,
            budget_tokens: None,
            options: HashMap::new(),
        }
    }

    pub fn low() -> Self {
        Self {
            effort: ReasoningEffort::Low,
            ..Self::none()
        }
    }

    pub fn medium() -> Self {
        Self {
            effort: ReasoningEffort::Medium,
            ..Self::none()
        }
    }

    pub fn high() -> Self {
        Self {
            effort: ReasoningEffort::High,
            ..Self::none()
        }
    }

    pub fn xhigh() -> Self {
        Self {
            effort: ReasoningEffort::XHigh,
            ..Self::none()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.effort != ReasoningEffort::None
    }

    pub fn with_budget(effort: ReasoningEffort, budget: i32) -> Self {
        Self {
            effort,
            budget_tokens: Some(budget),
            options: HashMap::new(),
        }
    }
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        Self::none()
    }
}

/// Parses effort name only (no budget/options).
/// "high" → ThinkingLevel::high(), "none" → ThinkingLevel::none()
impl FromStr for ThinkingLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let effort: ReasoningEffort = s.parse()?;
        Ok(Self {
            effort,
            budget_tokens: None,
            options: HashMap::new(),
        })
    }
}

/// Reasoning effort level. Ordered from lowest to highest.
/// Provider-agnostic scale — thinking_convert maps to per-provider values.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    #[default]
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl FromStr for ReasoningEffort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" | "x_high" | "max" => Ok(Self::XHigh),
            _ => Err(format!("unknown reasoning effort: {s}")),
        }
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
#[path = "thinking.test.rs"]
mod tests;
