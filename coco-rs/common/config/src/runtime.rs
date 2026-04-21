use std::collections::HashMap;
use std::path::PathBuf;

use coco_types::ModelRole;
use coco_types::ModelSpec;

use crate::env::EnvOnlyConfig;
use crate::env::EnvSnapshot;
use crate::model::ModelRoles;
use crate::model::ModelSelection;
use crate::overrides::RuntimeOverrides;
use crate::provider::ProviderConfig;
use crate::provider::builtin::builtin_providers;
use crate::sections::ApiConfig;
use crate::sections::LoopConfig;
use crate::sections::McpRuntimeConfig;
use crate::sections::MemoryConfig;
use crate::sections::PathConfig;
use crate::sections::SandboxConfig;
use crate::sections::ShellConfig;
use crate::sections::ToolConfig;
use crate::sections::WebFetchConfig;
use crate::sections::WebSearchConfig;
use crate::settings::SettingsWithSource;

/// JSON-first runtime configuration snapshot.
///
/// This is the boundary object leaf crates should consume instead of reading
/// process env or reconstructing defaults themselves. The raw env snapshot
/// is intentionally not retained — every env-derived knob has already been
/// folded into either `env_only` or a typed section, so consumers never
/// need to reach back at the unfiltered snapshot.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub settings: SettingsWithSource,
    pub env_only: EnvOnlyConfig,
    pub overrides: RuntimeOverrides,
    pub providers: HashMap<String, ProviderConfig>,
    pub model_roles: ModelRoles,
    pub api: ApiConfig,
    pub loop_config: LoopConfig,
    pub tool: ToolConfig,
    pub shell: ShellConfig,
    pub sandbox: SandboxConfig,
    pub memory: MemoryConfig,
    pub mcp: McpRuntimeConfig,
    pub web_fetch: WebFetchConfig,
    pub web_search: WebSearchConfig,
    pub paths: PathConfig,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfigBuilder {
    cwd: PathBuf,
    flag_settings: Option<PathBuf>,
    env: EnvSnapshot,
    overrides: RuntimeOverrides,
}

impl RuntimeConfigBuilder {
    pub fn from_process(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            flag_settings: None,
            env: EnvSnapshot::from_current_process(),
            overrides: RuntimeOverrides::default(),
        }
    }

    pub fn new(cwd: impl Into<PathBuf>, env: EnvSnapshot) -> Self {
        Self {
            cwd: cwd.into(),
            flag_settings: None,
            env,
            overrides: RuntimeOverrides::default(),
        }
    }

    pub fn with_flag_settings(mut self, path: impl Into<PathBuf>) -> Self {
        self.flag_settings = Some(path.into());
        self
    }

    pub fn with_overrides(mut self, overrides: RuntimeOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn build(self) -> anyhow::Result<RuntimeConfig> {
        let settings = crate::settings::load_settings(&self.cwd, self.flag_settings.as_deref())?;
        build_runtime_config(settings, self.env, self.overrides)
    }
}

fn build_runtime_config(
    settings: SettingsWithSource,
    env: EnvSnapshot,
    overrides: RuntimeOverrides,
) -> anyhow::Result<RuntimeConfig> {
    let env_only = EnvOnlyConfig::from_snapshot(&env);
    let providers = resolve_providers(&settings);
    let model_roles = resolve_model_roles(&settings, &env_only, &overrides, &providers)?;
    let merged = &settings.merged;

    Ok(RuntimeConfig {
        api: ApiConfig::resolve(merged),
        loop_config: LoopConfig::resolve(merged, &overrides),
        tool: ToolConfig::resolve(merged, &env),
        shell: ShellConfig::resolve(merged, &env),
        sandbox: SandboxConfig::resolve(merged, &env),
        memory: MemoryConfig::resolve(merged, &env),
        mcp: McpRuntimeConfig::resolve(merged, &env),
        web_fetch: WebFetchConfig::resolve(merged),
        web_search: WebSearchConfig::resolve(merged),
        paths: PathConfig::resolve(merged),
        settings,
        env_only,
        overrides,
        providers,
        model_roles,
    })
}

fn resolve_providers(settings: &SettingsWithSource) -> HashMap<String, ProviderConfig> {
    let mut providers = HashMap::new();
    for provider in builtin_providers() {
        providers.insert(provider.name.clone(), provider);
    }

    for (name, override_cfg) in &settings.merged.providers {
        let mut merged = providers
            .remove(name)
            .unwrap_or_else(ProviderConfig::default);
        merged.merge_from(override_cfg);
        if merged.name.is_empty() {
            merged.name.clone_from(name);
        }
        providers.insert(name.clone(), merged);
    }

    providers
}

fn resolve_model_roles(
    settings: &SettingsWithSource,
    env: &EnvOnlyConfig,
    overrides: &RuntimeOverrides,
    providers: &HashMap<String, ProviderConfig>,
) -> anyhow::Result<ModelRoles> {
    let mut roles = ModelRoles::default();
    let main_model = if let Some(selection) = overrides.model_override.as_deref() {
        model_spec_from_selection(selection, providers)?
    } else if let Some(selection) = env.model_override.as_deref() {
        model_spec_from_selection(selection, providers)?
    } else if let Some(selection) = settings.merged.models.main.clone() {
        resolve_model_selection(selection, providers)?
    } else if let Some(selection) = settings.merged.model.as_deref() {
        model_spec_from_selection(selection, providers)?
    } else {
        default_main_model_spec(providers)?
    };

    roles.roles.insert(ModelRole::Main, main_model);

    set_role_from_json(
        &mut roles,
        ModelRole::Fast,
        settings.merged.models.fast.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::Compact,
        settings.merged.models.compact.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::Plan,
        settings.merged.models.plan.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::Explore,
        settings.merged.models.explore.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::Review,
        settings.merged.models.review.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::HookAgent,
        settings.merged.models.hook_agent.as_ref(),
        providers,
    )?;
    set_role_from_json(
        &mut roles,
        ModelRole::Memory,
        settings.merged.models.memory.as_ref(),
        providers,
    )?;

    if let Some(model) = env.small_fast_model.as_deref() {
        roles.roles.insert(
            ModelRole::Fast,
            model_spec_from_selection(model, providers)?,
        );
    }
    if let Some(model) = env.subagent_model.as_deref() {
        roles.roles.insert(
            ModelRole::Explore,
            model_spec_from_selection(model, providers)?,
        );
    }

    Ok(roles)
}

fn set_role_from_json(
    roles: &mut ModelRoles,
    role: ModelRole,
    selection: Option<&ModelSelection>,
    providers: &HashMap<String, ProviderConfig>,
) -> anyhow::Result<()> {
    if let Some(selection) = selection {
        roles
            .roles
            .insert(role, resolve_model_selection(selection.clone(), providers)?);
    }
    Ok(())
}

fn model_spec_from_selection(
    selection: &str,
    providers: &HashMap<String, ProviderConfig>,
) -> anyhow::Result<ModelSpec> {
    let (provider_name, model_id) = selection.split_once('/').ok_or_else(|| {
        anyhow::anyhow!(
            "model selection `{selection}` must use explicit `provider/model_id` format"
        )
    })?;
    if provider_name.is_empty() || model_id.is_empty() {
        anyhow::bail!("model selection `{selection}` must use explicit `provider/model_id` format");
    }
    let provider = providers.get(provider_name).ok_or_else(|| {
        anyhow::anyhow!(
            "model selection `{selection}` references unknown provider `{provider_name}`"
        )
    })?;
    Ok(ModelSpec {
        provider: provider_name.to_string(),
        api: provider.api,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
    })
}

fn resolve_model_selection(
    selection: ModelSelection,
    providers: &HashMap<String, ProviderConfig>,
) -> anyhow::Result<ModelSpec> {
    if selection.provider.is_empty() || selection.model_id.is_empty() {
        anyhow::bail!("model role selection must include non-empty `provider` and `model_id`");
    }
    let provider = providers.get(&selection.provider).ok_or_else(|| {
        anyhow::anyhow!(
            "model `{}` references unknown provider `{}`",
            selection.model_id,
            selection.provider
        )
    })?;
    Ok(selection.into_model_spec(provider.api))
}

fn default_main_model_spec(
    providers: &HashMap<String, ProviderConfig>,
) -> anyhow::Result<ModelSpec> {
    // Iterate in a stable order so the fallback is deterministic even when
    // `providers` is a `HashMap`: builtins first (in their defined order),
    // then any user-registered extras sorted by name.
    //
    // Intentionally does **not** probe `resolve_api_key()` — that reads the
    // live process env and would break the `EnvSnapshot`-based determinism
    // of `build_runtime_config` (provider env-key names are arbitrary
    // strings and are not captured in the snapshot). Users who want a
    // specific provider as main must set `settings.model`, `settings.models.main`,
    // `COCO_MODEL`, or `RuntimeOverrides::model_override`.
    let builtin_order: Vec<String> = builtin_providers().into_iter().map(|p| p.name).collect();
    let mut ordered: Vec<&ProviderConfig> = Vec::with_capacity(providers.len());
    for name in &builtin_order {
        if let Some(p) = providers.get(name) {
            ordered.push(p);
        }
    }
    let mut extras: Vec<&ProviderConfig> = providers
        .iter()
        .filter(|(n, _)| !builtin_order.contains(n))
        .map(|(_, p)| p)
        .collect();
    extras.sort_by(|a, b| a.name.cmp(&b.name));
    ordered.extend(extras);

    let pick = ordered
        .into_iter()
        .find(|p| p.default_model.is_some())
        .or_else(|| providers.get("anthropic"))
        .ok_or_else(|| {
            anyhow::anyhow!("no provider with a configured default model is available")
        })?;
    let model_id = pick
        .default_model
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("default provider `{}` has no default model", pick.name))?;
    Ok(ModelSpec {
        provider: pick.name.clone(),
        api: pick.api,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
    })
}

#[cfg(test)]
#[path = "runtime.test.rs"]
mod tests;
