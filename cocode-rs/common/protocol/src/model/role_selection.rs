//! Runtime role selection types.
//!
//! Defines `RoleSelection` (model + thinking level for a single role) and
//! `RoleSelections` (all roles' runtime selections).

use std::collections::BTreeMap;

use super::Capability;
use super::ModelRole;
use super::ModelSpec;
use crate::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;

/// Runtime selection for a single role: current model + current thinking level.
///
/// `RoleSelection` is used for runtime state (in-memory switching), distinct from
/// `ModelRoles` which is used for JSON configuration (persisted to disk).
///
/// # Example
///
/// ```
/// use cocode_protocol::model::{RoleSelection, ModelSpec};
/// use cocode_protocol::ThinkingLevel;
///
/// // Create with just model
/// let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"));
///
/// // Create with model + thinking level
/// let selection = RoleSelection::with_thinking(
///     ModelSpec::new("anthropic", "claude-opus-4"),
///     ThinkingLevel::high(),
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleSelection {
    /// Current model (provider/model).
    pub model: ModelSpec,

    /// Current thinking level (overrides ModelInfo.default_thinking_level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,

    /// Thinking levels this model supports (for UI cycling). From ModelInfo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_thinking_levels: Option<Vec<ThinkingLevel>>,

    /// Model capabilities (for UI gating). From ModelInfo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<Capability>>,
}

impl RoleSelection {
    /// Create a new role selection with only the model.
    pub fn new(model: ModelSpec) -> Self {
        Self {
            model,
            thinking_level: None,
            supported_thinking_levels: None,
            capabilities: None,
        }
    }

    /// Create a role selection with model and thinking level.
    pub fn with_thinking(model: ModelSpec, level: ThinkingLevel) -> Self {
        Self {
            model,
            thinking_level: Some(level),
            supported_thinking_levels: None,
            capabilities: None,
        }
    }

    /// Create from a ModelSpec (uses model's default thinking level).
    pub fn from_spec(spec: ModelSpec) -> Self {
        Self::new(spec)
    }

    /// Set supported thinking levels (for UI cycling).
    pub fn with_supported_thinking_levels(mut self, levels: Vec<ThinkingLevel>) -> Self {
        self.supported_thinking_levels = Some(levels);
        self
    }

    /// Set capabilities (for UI gating).
    pub fn with_capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.capabilities = Some(caps);
        self
    }

    /// Check whether this model supports a specific capability.
    pub fn has_capability(&self, cap: Capability) -> bool {
        self.capabilities.as_ref().is_some_and(|c| c.contains(&cap))
    }

    /// Effective thinking level: explicit override or default (None effort).
    pub fn effective_thinking_level(&self) -> ThinkingLevel {
        self.thinking_level.clone().unwrap_or_default()
    }

    /// Update the thinking level.
    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = Some(level);
    }

    /// Clear the thinking level override.
    pub fn clear_thinking_level(&mut self) {
        self.thinking_level = None;
    }

    /// Get the provider name.
    pub fn provider(&self) -> &str {
        &self.model.provider
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.model.slug
    }
}

/// All roles' runtime selections.
///
/// Unlike `ModelRoles` which uses `ModelSpec` for JSON config, `RoleSelections`
/// uses `RoleSelection` which includes the current thinking level override.
///
/// # Example
///
/// ```
/// use cocode_protocol::model::{RoleSelections, RoleSelection, ModelRole, ModelSpec};
/// use cocode_protocol::ThinkingLevel;
///
/// let mut selections = RoleSelections::default();
///
/// // Set main role
/// selections.set(
///     ModelRole::Main,
///     RoleSelection::with_thinking(
///         ModelSpec::new("anthropic", "claude-opus-4"),
///         ThinkingLevel::high(),
///     ),
/// );
///
/// // Set fast role
/// selections.set(
///     ModelRole::Fast,
///     RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
/// );
///
/// // Get selection for a role
/// if let Some(main) = selections.get(ModelRole::Main) {
///     println!("Main: {}", main.model);
/// }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoleSelections(BTreeMap<ModelRole, RoleSelection>);

impl RoleSelections {
    /// Create empty selections.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with only the main role set.
    pub fn with_main(selection: RoleSelection) -> Self {
        Self(BTreeMap::from([(ModelRole::Main, selection)]))
    }

    /// Get selection for a specific role.
    pub fn get(&self, role: ModelRole) -> Option<&RoleSelection> {
        self.0.get(&role)
    }

    /// Get mutable selection for a specific role.
    pub fn get_mut(&mut self, role: ModelRole) -> Option<&mut RoleSelection> {
        self.0.get_mut(&role)
    }

    /// Set selection for a specific role.
    pub fn set(&mut self, role: ModelRole, selection: RoleSelection) {
        self.0.insert(role, selection);
    }

    /// Clear selection for a specific role.
    pub fn clear(&mut self, role: ModelRole) {
        self.0.remove(&role);
    }

    /// Check if any role is selected.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get selection for a role, falling back to main if not set.
    pub fn get_or_main(&self, role: ModelRole) -> Option<&RoleSelection> {
        self.0.get(&role).or_else(|| self.0.get(&ModelRole::Main))
    }

    /// Update thinking level for a specific role.
    ///
    /// Returns `true` if the role selection exists and was updated.
    pub fn set_thinking_level(&mut self, role: ModelRole, level: ThinkingLevel) -> bool {
        if let Some(selection) = self.get_mut(role) {
            selection.set_thinking_level(level);
            true
        } else {
            false
        }
    }

    /// Merge another RoleSelections into this one.
    ///
    /// Values from `other` take precedence where set.
    pub fn merge(&mut self, other: &RoleSelections) {
        for (role, selection) in &other.0 {
            self.0.insert(*role, selection.clone());
        }
    }
}

#[cfg(test)]
#[path = "role_selection.test.rs"]
mod tests;
