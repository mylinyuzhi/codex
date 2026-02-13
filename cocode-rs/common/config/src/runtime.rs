//! Runtime state management for provider/model switching.
//!
//! This module encapsulates runtime-time configuration changes, including
//! switching between providers, models, and adjusting thinking levels per role.
//! These overrides are stored in-memory and take highest precedence in
//! configuration resolution.

use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use std::sync::RwLock;

/// Manages runtime state for provider/model selection and thinking levels.
///
/// RuntimeState tracks:
/// - Per-role model selections
/// - Per-role thinking level overrides
/// - Changes made during the session
///
/// These are in-memory only and take precedence over file-based configuration.
#[derive(Debug)]
pub struct RuntimeState {
    /// Role-specific selections (model + thinking_level per role).
    selections: RwLock<RoleSelections>,
}

impl RuntimeState {
    /// Create a new runtime state with no overrides.
    pub fn new() -> Self {
        Self {
            selections: RwLock::new(RoleSelections::default()),
        }
    }

    /// Get the model spec for a specific role.
    ///
    /// Returns `Some` if an override is set for this role, `None` if using defaults.
    pub fn current_spec(&self, role: ModelRole) -> Option<ModelSpec> {
        self.selections
            .read()
            .unwrap()
            .get(role)
            .map(|s| s.model.clone())
    }

    /// Get the model spec for the Main role (convenience method).
    pub fn current_spec_main(&self) -> Option<ModelSpec> {
        self.current_spec(ModelRole::Main)
    }

    /// Switch the model for a specific role.
    ///
    /// Updates the in-memory override for this role.
    pub fn switch_spec(&self, role: ModelRole, spec: &ModelSpec) {
        let mut selections = self.selections.write().unwrap();
        selections.set(role, RoleSelection::new(spec.clone()));
    }

    /// Switch the model for the Main role (convenience method).
    pub fn switch_spec_main(&self, spec: &ModelSpec) {
        self.switch_spec(ModelRole::Main, spec);
    }

    /// Get the thinking level for a specific role.
    pub fn thinking_level(&self, role: ModelRole) -> Option<ThinkingLevel> {
        self.selections
            .read()
            .unwrap()
            .get(role)
            .and_then(|s| s.thinking_level.clone())
    }

    /// Set the thinking level for a specific role.
    pub fn set_thinking_level(&self, role: ModelRole, level: ThinkingLevel) {
        let mut selections = self.selections.write().unwrap();
        if let Some(selection) = selections.get_mut(role) {
            selection.thinking_level = Some(level);
        } else {
            let selection = RoleSelection::new(ModelSpec::new("", ""));
            let mut new_selection = selection;
            new_selection.thinking_level = Some(level);
            selections.set(role, new_selection);
        }
    }

    /// Clear the override for a specific role.
    pub fn clear_role(&self, role: ModelRole) {
        let mut selections = self.selections.write().unwrap();
        selections.clear(role);
    }

    /// Get all current selections.
    pub fn all_selections(&self) -> RoleSelections {
        self.selections.read().unwrap().clone()
    }

    /// Get the selection for a specific role (includes both model and thinking level).
    pub fn get_selection(&self, role: ModelRole) -> Option<RoleSelection> {
        self.selections.read().unwrap().get(role).cloned()
    }

    // === Deprecated String-Based APIs (Phase 2C will remove these) ===

    /// Get the main model override (for backward compatibility).
    ///
    /// **Deprecated**: Use `current_spec_main()` instead.
    pub fn main(&self) -> Option<ModelSpec> {
        self.selections
            .read()
            .unwrap()
            .get(ModelRole::Main)
            .map(|s| s.model.clone())
    }

    /// Set the main model override (for backward compatibility).
    ///
    /// **Deprecated**: Use `switch_spec_main()` instead.
    pub fn set_main(&self, spec: ModelSpec) {
        self.switch_spec_main(&spec);
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_state_default() {
        let state = RuntimeState::new();
        assert!(state.current_spec_main().is_none());
    }

    #[test]
    fn test_switch_spec_main() {
        let state = RuntimeState::new();
        let spec = ModelSpec::new("anthropic", "claude-opus-4");
        state.switch_spec_main(&spec);
        assert_eq!(state.current_spec_main().unwrap(), spec);
    }

    #[test]
    fn test_clear_role() {
        let state = RuntimeState::new();
        let spec = ModelSpec::new("anthropic", "claude-opus-4");
        state.switch_spec_main(&spec);
        assert!(state.current_spec_main().is_some());
        state.clear_role(ModelRole::Main);
        assert!(state.current_spec_main().is_none());
    }

    #[test]
    fn test_backward_compat_main() {
        let state = RuntimeState::new();
        let spec = ModelSpec::new("anthropic", "claude-sonnet-4");
        state.set_main(spec.clone());
        assert_eq!(state.main(), Some(spec));
    }
}
