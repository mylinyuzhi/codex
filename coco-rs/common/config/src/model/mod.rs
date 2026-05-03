pub mod aliases;
mod instructions;
pub mod partial;
pub mod registry;
pub mod role_slots;

pub use partial::PartialModelInfo;
pub use registry::ModelRegistry;
pub use registry::ResolvedModel;
pub use registry::build_model_registry;
pub use role_slots::FallbackRecoveryPolicy;
pub use role_slots::RoleSlots;

use crate::error::ConfigError;
use crate::error::ConfigField;
use crate::positive::PositiveCount;
use crate::positive::PositiveTokens;
use coco_types::ApplyPatchToolType;
use coco_types::Capability;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use coco_types::ToolOverrides;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Resolved per-model configuration. The on-disk overlay shape is
/// [`PartialModelInfo`]; this is the post-resolution form with required
/// fields concrete.
///
/// Required fields (`context_window`, `max_output_tokens`) are typed
/// `PositiveTokens` so that `as u64` casts are unrepresentable in the
/// downstream call chain — `From<PositiveTokens> for u64` is infallible.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    // === Identity ===
    pub model_id: String,
    pub display_name: Option<String>,

    // === Capacity ===
    pub context_window: PositiveTokens,
    pub max_output_tokens: PositiveTokens,
    pub timeout_secs: Option<i64>,

    // === Capabilities ===
    pub capabilities: Option<Vec<Capability>>,

    // === Sampling — `Option` carries wire semantics ("let provider default") ===
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<PositiveCount>,

    // === Thinking / Reasoning ===
    pub supported_thinking_levels: Option<Vec<ThinkingLevel>>,
    pub default_thinking_level: Option<ReasoningEffort>,

    // === Context Management ===
    pub auto_compact_pct: Option<i32>,

    // === Tools ===
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    /// Per-model tool-availability adjustments. Layered on top of the
    /// built-in registry. See `docs/coco-rs/feature-gates-and-tool-filtering.md`.
    pub tool_overrides: Option<ToolOverrides>,
    pub shell_type: Option<String>,
    pub max_tool_output_chars: Option<i32>,

    // === Instructions ===
    pub base_instructions: Option<String>,
    pub base_instructions_file: Option<String>,

    /// Layer 1 escape hatch. Provider-agnostic flat keys, **camelCase**
    /// to match each provider's typed-options struct (Layer 3 reads
    /// `#[serde(rename_all = "camelCase")]`). Layer 2 wraps as
    /// `provider_options[<provider_name>]` at call time. snake_case keys
    /// silently fall through Layer 3's typed parser to leftover-merge.
    pub extra_body: BTreeMap<String, serde_json::Value>,
}

impl Default for ModelInfo {
    /// Sentinel placeholder for tests and in-process construction. The
    /// `context_window` / `max_output_tokens` values are arbitrary —
    /// they bypass the JSON-boundary validation enforced by
    /// [`ModelInfo::from_partial`].
    ///
    /// **Production paths must not use `Default::default()`** —
    /// always go through `from_partial(provider, model_id, partial)`
    /// so missing required fields surface as
    /// `ConfigError::IncompleteModelEntry { ContextWindow | MaxOutputTokens }`
    /// rather than silently passing through.
    fn default() -> Self {
        Self {
            model_id: String::new(),
            display_name: None,
            context_window: PositiveTokens::new(200_000),
            max_output_tokens: PositiveTokens::new(16_384),
            timeout_secs: None,
            capabilities: None,
            temperature: None,
            top_p: None,
            top_k: None,
            supported_thinking_levels: None,
            default_thinking_level: None,
            auto_compact_pct: None,
            apply_patch_tool_type: None,
            tool_overrides: None,
            shell_type: None,
            max_tool_output_chars: None,
            base_instructions: None,
            base_instructions_file: None,
            extra_body: BTreeMap::new(),
        }
    }
}

impl ModelInfo {
    /// Resolve a `PartialModelInfo` into a complete `ModelInfo`. The
    /// only public path from JSON; surfaces a typed error when a
    /// required field never appeared anywhere in the merge chain.
    pub fn from_partial(
        provider: &str,
        model_id: &str,
        partial: PartialModelInfo,
    ) -> Result<Self, ConfigError> {
        Ok(Self {
            model_id: model_id.to_string(),
            display_name: partial.display_name,
            context_window: partial.context_window.ok_or_else(|| {
                ConfigError::IncompleteModelEntry {
                    provider: provider.to_string(),
                    model: model_id.to_string(),
                    field: ConfigField::ContextWindow,
                }
            })?,
            max_output_tokens: partial.max_output_tokens.ok_or_else(|| {
                ConfigError::IncompleteModelEntry {
                    provider: provider.to_string(),
                    model: model_id.to_string(),
                    field: ConfigField::MaxOutputTokens,
                }
            })?,
            timeout_secs: partial.timeout_secs,
            capabilities: partial.capabilities,
            temperature: partial.temperature,
            top_p: partial.top_p,
            top_k: partial.top_k,
            supported_thinking_levels: partial.supported_thinking_levels,
            default_thinking_level: partial.default_thinking_level,
            auto_compact_pct: partial.auto_compact_pct,
            apply_patch_tool_type: partial.apply_patch_tool_type,
            tool_overrides: partial.tool_overrides,
            shell_type: partial.shell_type,
            max_tool_output_chars: partial.max_tool_output_chars,
            base_instructions: partial.base_instructions,
            base_instructions_file: partial.base_instructions_file,
            extra_body: partial.extra_body.unwrap_or_default(),
        })
    }

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
    ///
    /// Resolution semantics:
    /// - `Some(non-empty)` — exact-effort match wins; otherwise fall
    ///   back to the closest declared level by effort distance.
    /// - `None` (field absent) — pass `requested` through unchanged;
    ///   the model has not declared its thinking surface, so trust
    ///   the caller.
    /// - `Some(vec![])` — also passes `requested` through. An
    ///   explicitly-empty list is treated as "no declared surface,"
    ///   identical to `None`. If a future caller needs an explicit
    ///   "thinking unsupported" signal, prefer omitting `Capability::ExtendedThinking`
    ///   from `capabilities` rather than overloading this field.
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
#[derive(Debug, Clone, Default)]
pub struct ModelRoles {
    pub roles: HashMap<ModelRole, RoleSlots<ModelSpec>>,
}

impl ModelRoles {
    /// Primary model for a role. Falls back to `Main`'s primary if
    /// the role is unset.
    pub fn get(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.roles
            .get(&role)
            .map(|s| &s.primary)
            .or_else(|| self.roles.get(&ModelRole::Main).map(|s| &s.primary))
    }

    /// Ordered fallback chain for a role. Strictly per-role.
    pub fn fallbacks(&self, role: ModelRole) -> &[ModelSpec] {
        self.roles
            .get(&role)
            .map(|s| s.fallbacks.as_slice())
            .unwrap_or(&[])
    }

    /// Recovery policy for a role. `None` = sticky.
    pub fn recovery(&self, role: ModelRole) -> Option<FallbackRecoveryPolicy> {
        self.roles.get(&role).and_then(|s| s.recovery)
    }

    /// Full `RoleSlots` for a role.
    pub fn role_slots(&self, role: ModelRole) -> Option<&RoleSlots<ModelSpec>> {
        self.roles.get(&role)
    }
}

/// JSON-facing role model selection.
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
    /// Forked-agent spawn model (TS `tools/AgentTool/`). Generic role
    /// for agent/skill subagent dispatch — distinct from `explore`,
    /// which is the investigative subagent type.
    pub subagent: Option<RoleSlots<ModelSelection>>,
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
