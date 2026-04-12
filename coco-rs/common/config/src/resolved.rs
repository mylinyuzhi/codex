use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use coco_types::ThinkingLevel;

use crate::env::EnvOnlyConfig;
use crate::model::ModelRoles;
use crate::overrides::RuntimeOverrides;
use crate::settings::SettingsWithSource;

/// The fully resolved configuration for a session.
/// Combines: persisted Settings + EnvOnlyConfig + RuntimeOverrides.
pub struct ResolvedConfig {
    pub settings: SettingsWithSource,
    pub env: EnvOnlyConfig,
    pub overrides: RuntimeOverrides,
    pub model_roles: ModelRoles,
}

impl ResolvedConfig {
    /// Get the effective API provider.
    pub fn api_provider(&self) -> ProviderApi {
        if self.env.use_bedrock || self.env.use_vertex || self.env.use_foundry {
            return ProviderApi::Anthropic;
        }
        // Default
        ProviderApi::Anthropic
    }

    /// Get the effective model for a role.
    pub fn model_for_role(&self, role: ModelRole) -> Option<&ModelSpec> {
        self.model_roles.get(role)
    }

    /// Get the effective thinking level.
    /// Priority: override > settings > model default
    pub fn effective_thinking_level(&self) -> Option<&ThinkingLevel> {
        if let Some(ref level) = self.overrides.thinking_level_override {
            return Some(level);
        }
        self.settings.merged.thinking_level.as_ref()
    }

    /// Is fast mode active?
    pub fn is_fast_mode(&self) -> bool {
        self.overrides
            .fast_mode_override
            .or(self.settings.merged.fast_mode)
            .unwrap_or(false)
    }
}
