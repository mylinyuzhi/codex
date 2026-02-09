//! Multi-model role configuration.

use super::ModelSpec;
use serde::Deserialize;
use serde::Serialize;

/// Model role identifier.
///
/// Different roles allow using specialized models for specific tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    /// Primary model for main interactions.
    Main,
    /// Fast model for quick operations (cheaper/faster).
    Fast,
    /// Vision-capable model for image analysis.
    Vision,
    /// Model for code review tasks.
    Review,
    /// Model for planning and architecture.
    Plan,
    /// Model for codebase exploration.
    Explore,
    /// Model for context compaction and summarization.
    Compact,
}

impl ModelRole {
    /// Get all available roles.
    pub fn all() -> &'static [ModelRole] {
        &[
            ModelRole::Main,
            ModelRole::Fast,
            ModelRole::Vision,
            ModelRole::Review,
            ModelRole::Plan,
            ModelRole::Explore,
            ModelRole::Compact,
        ]
    }

    /// Get the role name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelRole::Main => "main",
            ModelRole::Fast => "fast",
            ModelRole::Vision => "vision",
            ModelRole::Review => "review",
            ModelRole::Plan => "plan",
            ModelRole::Explore => "explore",
            ModelRole::Compact => "compact",
        }
    }
}

impl std::fmt::Display for ModelRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Multi-model configuration with role-based fallback.
///
/// All roles are optional. When a role is not set, it falls back to `main`.
///
/// # Example
///
/// ```
/// use cocode_protocol::model::{ModelRoles, ModelRole, ModelSpec};
///
/// let roles: ModelRoles = serde_json::from_str(r#"{
///     "main": "anthropic/claude-opus-4",
///     "fast": "anthropic/claude-haiku"
/// }"#).unwrap();
///
/// // Fast role returns the configured model
/// assert_eq!(roles.get(ModelRole::Fast).unwrap().model, "claude-haiku");
///
/// // Vision role falls back to main (not configured)
/// assert_eq!(roles.get(ModelRole::Vision).unwrap().model, "claude-opus-4");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRoles {
    /// Primary model for main interactions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<ModelSpec>,

    /// Fast model for quick operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<ModelSpec>,

    /// Vision-capable model for image analysis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<ModelSpec>,

    /// Model for code review tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ModelSpec>,

    /// Model for planning and architecture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<ModelSpec>,

    /// Model for codebase exploration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explore: Option<ModelSpec>,

    /// Model for context compaction and summarization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<ModelSpec>,
}

impl ModelRoles {
    /// Create a new ModelRoles with only the main model set.
    pub fn with_main(main: ModelSpec) -> Self {
        Self {
            main: Some(main),
            ..Default::default()
        }
    }

    /// Get model for a specific role, falling back to main if not set.
    pub fn get(&self, role: ModelRole) -> Option<&ModelSpec> {
        let specific = match role {
            ModelRole::Main => &self.main,
            ModelRole::Fast => &self.fast,
            ModelRole::Vision => &self.vision,
            ModelRole::Review => &self.review,
            ModelRole::Plan => &self.plan,
            ModelRole::Explore => &self.explore,
            ModelRole::Compact => &self.compact,
        };
        specific.as_ref().or(self.main.as_ref())
    }

    /// Get the main model directly (no fallback).
    pub fn main(&self) -> Option<&ModelSpec> {
        self.main.as_ref()
    }

    /// Check if any model is configured.
    pub fn is_empty(&self) -> bool {
        self.main.is_none()
            && self.fast.is_none()
            && self.vision.is_none()
            && self.review.is_none()
            && self.plan.is_none()
            && self.explore.is_none()
            && self.compact.is_none()
    }

    /// Set a model for a specific role.
    pub fn set(&mut self, role: ModelRole, spec: ModelSpec) {
        match role {
            ModelRole::Main => self.main = Some(spec),
            ModelRole::Fast => self.fast = Some(spec),
            ModelRole::Vision => self.vision = Some(spec),
            ModelRole::Review => self.review = Some(spec),
            ModelRole::Plan => self.plan = Some(spec),
            ModelRole::Explore => self.explore = Some(spec),
            ModelRole::Compact => self.compact = Some(spec),
        }
    }

    /// Merge another ModelRoles into this one.
    ///
    /// Values from `other` take precedence where set.
    pub fn merge(&mut self, other: &ModelRoles) {
        if other.main.is_some() {
            self.main = other.main.clone();
        }
        if other.fast.is_some() {
            self.fast = other.fast.clone();
        }
        if other.vision.is_some() {
            self.vision = other.vision.clone();
        }
        if other.review.is_some() {
            self.review = other.review.clone();
        }
        if other.plan.is_some() {
            self.plan = other.plan.clone();
        }
        if other.explore.is_some() {
            self.explore = other.explore.clone();
        }
        if other.compact.is_some() {
            self.compact = other.compact.clone();
        }
    }
}

#[cfg(test)]
#[path = "model_roles.test.rs"]
mod tests;
