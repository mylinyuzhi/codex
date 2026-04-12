pub mod aliases;

use coco_types::ApplyPatchToolType;
use coco_types::Capability;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::ResolvedConfig;

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

/// Role -> model mapping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRoles {
    pub roles: HashMap<ModelRole, ModelSpec>,
}

impl ModelRoles {
    /// Get the model spec for a role, falling back to Main.
    pub fn get(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.roles
            .get(&role)
            .or_else(|| self.roles.get(&ModelRole::Main))
    }
}

/// Model selection: priority-based resolution.
/// 1. RuntimeOverrides.model_override (/model command)
/// 2. EnvOnlyConfig.model_override (ANTHROPIC_MODEL env)
/// 3. Settings.model (merged config file field)
/// 4. Default by provider
pub fn get_main_loop_model(config: &ResolvedConfig) -> String {
    if let Some(ref m) = config.overrides.model_override {
        return m.clone();
    }
    if let Some(ref m) = config.env.model_override {
        return m.clone();
    }
    if let Some(ref m) = config.settings.merged.model {
        return m.clone();
    }
    // Default: Claude Sonnet
    "claude-sonnet-4-6-20250514".into()
}

/// Subagent model resolution.
/// Priority: CLAUDE_CODE_SUBAGENT_MODEL env > tool_model > agent_model > parent.
/// TS: getAgentModel() in model.ts
pub fn get_agent_model(
    agent_model: Option<&str>,
    parent_spec: &ModelSpec,
    tool_model: Option<&str>,
    config: &ResolvedConfig,
) -> ModelSpec {
    // 1. Env override
    if let Some(ref m) = config.env.subagent_model {
        return ModelSpec {
            model_id: m.clone(),
            ..parent_spec.clone()
        };
    }
    // 2. Tool-specific model
    if let Some(m) = tool_model {
        return ModelSpec {
            model_id: m.to_string(),
            ..parent_spec.clone()
        };
    }
    // 3. Agent-specific model
    if let Some(m) = agent_model {
        return ModelSpec {
            model_id: m.to_string(),
            ..parent_spec.clone()
        };
    }
    // 4. Inherit from parent
    parent_spec.clone()
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
