//! Reasoning effort level types.

use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumIter;

/// Reasoning summary level for models that support it.
///
/// See <https://platform.openai.com/docs/guides/reasoning#reasoning-summaries>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningSummary {
    /// No reasoning summary.
    None,
    /// Auto (provider decides).
    #[default]
    Auto,
    /// Concise summary.
    Concise,
    /// Detailed summary.
    Detailed,
}

/// Reasoning effort level for models that support extended thinking.
///
/// Variants are ordered from lowest to highest effort, enabling direct comparison:
/// `ReasoningEffort::High > ReasoningEffort::Low`
///
/// See <https://platform.openai.com/docs/guides/reasoning>
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningEffort {
    /// No reasoning (ord = 0).
    None,
    /// Minimal reasoning (ord = 1).
    Minimal,
    /// Low reasoning effort (ord = 2).
    Low,
    /// Medium reasoning effort (ord = 3, default).
    #[default]
    Medium,
    /// High reasoning effort (ord = 4).
    High,
    /// Extra high reasoning effort (ord = 5).
    XHigh,
}

/// Find nearest supported effort level using `Ord` comparison.
pub fn nearest_effort(target: ReasoningEffort, supported: &[ReasoningEffort]) -> ReasoningEffort {
    supported
        .iter()
        .copied()
        .min_by_key(|c| (*c as i32 - target as i32).abs())
        .unwrap_or(target)
}

#[cfg(test)]
#[path = "reasoning.test.rs"]
mod tests;
