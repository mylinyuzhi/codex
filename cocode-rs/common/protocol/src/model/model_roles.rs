//! Multi-model role configuration.

use std::collections::BTreeMap;

use super::ModelSpec;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::IntoStaticStr;

/// Model role identifier.
///
/// Different roles allow using specialized models for specific tasks.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Display,
    IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
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
        (*self).into()
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
/// assert_eq!(roles.get(ModelRole::Fast).unwrap().slug, "claude-haiku");
///
/// // Vision role falls back to main (not configured)
/// assert_eq!(roles.get(ModelRole::Vision).unwrap().slug, "claude-opus-4");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelRoles(BTreeMap<ModelRole, ModelSpec>);

impl ModelRoles {
    /// Create a new ModelRoles with only the main model set.
    pub fn with_main(main: ModelSpec) -> Self {
        Self(BTreeMap::from([(ModelRole::Main, main)]))
    }

    /// Get model for a specific role, falling back to main if not set.
    pub fn get(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.0.get(&role).or_else(|| self.0.get(&ModelRole::Main))
    }

    /// Get model for a role WITHOUT falling back to main.
    pub fn get_direct(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.0.get(&role)
    }

    /// Get the main model directly (no fallback).
    pub fn main(&self) -> Option<&ModelSpec> {
        self.0.get(&ModelRole::Main)
    }

    /// Check if any model is configured.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Set a model for a specific role.
    pub fn set(&mut self, role: ModelRole, spec: ModelSpec) {
        self.0.insert(role, spec);
    }

    /// Merge another ModelRoles into this one.
    ///
    /// Values from `other` take precedence where set.
    pub fn merge(&mut self, other: &ModelRoles) {
        for (role, spec) in &other.0 {
            self.0.insert(*role, spec.clone());
        }
    }
}

#[cfg(test)]
#[path = "model_roles.test.rs"]
mod tests;
