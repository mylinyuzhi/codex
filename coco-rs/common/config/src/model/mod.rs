pub mod aliases;
pub mod role_slots;

pub use role_slots::FallbackRecoveryPolicy;
pub use role_slots::RoleSlots;

use coco_types::ApplyPatchToolType;
use coco_types::Capability;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Rich per-model configuration. All optional fields for layered merging.
///
/// Config layers (later overrides earlier):
///   builtin defaults → models.json → provider.models[model_id]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelInfo {
    // === Identity ===
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    // === Capacity ===
    /// NOT Option — every model has a context window (default 200_000).
    #[serde(default = "default_context_window")]
    pub context_window: i64,
    #[serde(default = "default_max_output")]
    pub max_output_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<i64>,

    // === Capabilities ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<Capability>>,

    // === Sampling ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,

    // === Thinking/Reasoning ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_thinking_levels: Option<Vec<ThinkingLevel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thinking_level: Option<ReasoningEffort>,

    // === Context Management ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compact_pct: Option<i32>,

    // === Tools ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_output_chars: Option<i32>,

    // === Instructions ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_instructions_file: Option<String>,

    // === Provider-Specific Extensions ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<HashMap<String, serde_json::Value>>,
}

fn default_context_window() -> i64 {
    200_000
}

fn default_max_output() -> i64 {
    16_384
}

impl ModelInfo {
    pub fn has_capability(&self, cap: Capability) -> bool {
        self.capabilities
            .as_ref()
            .is_some_and(|caps| caps.contains(&cap))
    }

    /// Get default ThinkingLevel by looking up default effort in supported levels.
    pub fn default_thinking(&self) -> Option<&ThinkingLevel> {
        let effort = self.default_thinking_level?;
        self.supported_thinking_levels
            .as_ref()?
            .iter()
            .find(|l| l.effort == effort)
    }

    /// Resolve a requested effort to the best matching supported ThinkingLevel.
    pub fn resolve_thinking_level(&self, requested: &ThinkingLevel) -> ThinkingLevel {
        match &self.supported_thinking_levels {
            Some(levels) if !levels.is_empty() => levels
                .iter()
                .find(|l| l.effort == requested.effort)
                .cloned()
                .unwrap_or_else(|| {
                    levels
                        .iter()
                        .min_by_key(|l| (l.effort as i32 - requested.effort as i32).abs())
                        .cloned()
                        .unwrap_or_else(|| requested.clone())
                }),
            _ => requested.clone(),
        }
    }

    /// Merge another config into this one (other.Some overrides self).
    pub fn merge_from(&mut self, other: &Self) {
        if !other.model_id.is_empty() {
            self.model_id.clone_from(&other.model_id);
        }
        macro_rules! merge_opt {
            ($field:ident) => {
                if other.$field.is_some() {
                    self.$field.clone_from(&other.$field);
                }
            };
        }
        merge_opt!(display_name);
        if other.context_window != default_context_window() {
            self.context_window = other.context_window;
        }
        if other.max_output_tokens != default_max_output() {
            self.max_output_tokens = other.max_output_tokens;
        }
        merge_opt!(timeout_secs);
        merge_opt!(capabilities);
        merge_opt!(temperature);
        merge_opt!(top_p);
        merge_opt!(top_k);
        merge_opt!(supported_thinking_levels);
        merge_opt!(default_thinking_level);
        merge_opt!(auto_compact_pct);
        merge_opt!(apply_patch_tool_type);
        merge_opt!(excluded_tools);
        merge_opt!(shell_type);
        merge_opt!(max_tool_output_chars);
        merge_opt!(base_instructions);
        merge_opt!(base_instructions_file);
        merge_opt!(options);
    }
}

impl PartialEq for ModelInfo {
    fn eq(&self, other: &Self) -> bool {
        self.model_id == other.model_id
    }
}

/// Role -> (primary + fallback chain + recovery policy).
///
/// The JSON-facing side uses `RoleSlots<ModelSelection>` (see
/// `ModelSelectionSettings`); this runtime-facing side stores the
/// already-resolved `RoleSlots<ModelSpec>`, produced by
/// `RuntimeConfigBuilder`.
///
/// Deliberately NOT `Deserialize` — it's runtime state built by the
/// resolver, never read from JSON. The config source is
/// `ModelSelectionSettings`.
#[derive(Debug, Clone, Default)]
pub struct ModelRoles {
    pub roles: HashMap<ModelRole, RoleSlots<ModelSpec>>,
}

impl ModelRoles {
    /// Primary model for a role. Falls back to `Main`'s primary if
    /// the role is unset — preserves the existing "any role without
    /// a dedicated model uses Main" behavior.
    pub fn get(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.roles
            .get(&role)
            .map(|s| &s.primary)
            .or_else(|| self.roles.get(&ModelRole::Main).map(|s| &s.primary))
    }

    /// Ordered fallback chain for a role. Strictly per-role — never
    /// walks to Main's fallbacks. Empty vec = no fallback configured
    /// for this role.
    pub fn fallbacks(&self, role: ModelRole) -> &[ModelSpec] {
        self.roles
            .get(&role)
            .map(|s| s.fallbacks.as_slice())
            .unwrap_or(&[])
    }

    /// Recovery policy for a role. `None` = sticky (stay on fallback
    /// once switched). Strictly per-role; no Main walk.
    pub fn recovery(&self, role: ModelRole) -> Option<FallbackRecoveryPolicy> {
        self.roles.get(&role).and_then(|s| s.recovery)
    }

    /// Full `RoleSlots` for a role. Used by the runtime-config
    /// resolver and tests that need the whole binding.
    pub fn role_slots(&self, role: ModelRole) -> Option<&RoleSlots<ModelSpec>> {
        self.roles.get(&role)
    }
}

/// JSON-facing role model selection.
///
/// This mirrors the selectable identity fields of `ModelSpec` without exposing
/// runtime dispatch or display fields; `RuntimeConfigBuilder` resolves provider
/// API from the provider catalog.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelSelection {
    pub provider: String,
    pub model_id: String,
}

impl ModelSelection {
    pub fn into_model_spec(self, api: ProviderApi) -> ModelSpec {
        ModelSpec {
            provider: self.provider,
            api,
            display_name: self.model_id.clone(),
            model_id: self.model_id,
        }
    }
}

/// JSON-facing role model selections.
///
/// Each role must name a provider explicitly. A role entry may also
/// carry a fallback chain (`fallback:` / `fallbacks:`) and optional
/// `recovery:` policy — see [`RoleSlots`] for the accepted shapes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSelectionSettings {
    pub main: Option<RoleSlots<ModelSelection>>,
    pub fast: Option<RoleSlots<ModelSelection>>,
    pub compact: Option<RoleSlots<ModelSelection>>,
    pub plan: Option<RoleSlots<ModelSelection>>,
    pub explore: Option<RoleSlots<ModelSelection>>,
    pub review: Option<RoleSlots<ModelSelection>>,
    pub hook_agent: Option<RoleSlots<ModelSelection>>,
    pub memory: Option<RoleSlots<ModelSelection>>,
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
