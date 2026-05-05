//! `ModelRegistry` — resolved `ModelInfo` indexed by `(provider, model_id)`.
//!
//! Built once per `RuntimeConfig` snapshot. Three-layer merge:
//!
//!   L0  builtin_models()                              (compile-time)
//!   L1  ~/.coco/models.json                           (per-machine catalog)
//!   L2  providers.<name>.models.<id>                  (per-(provider, model) entry)
//!
//! The result is `Arc<ResolvedModel>` so that downstream consumers
//! (model-factory, build_call_options, tool_overrides plumbing) can
//! Arc-clone without copying. New (provider, model) pairs that lack a
//! builtin/catalog still go through `from_partial` — an override
//! against an empty base is well-defined.

use crate::error::ConfigError;
use crate::model::ModelInfo;
use crate::model::instructions::builtin_base_instructions;
use crate::model::partial::PartialModelInfo;
use crate::provider::ProviderConfig;
use crate::provider::model_override::ProviderModelOverride;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Resolved (provider, model) entry — the per-call source of truth for
/// `build_call_options` and `model_factory`.
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub info: ModelInfo,
    pub provider_model: ProviderModelOverride,
}

/// Indexed registry of (provider, model) pairs, with lazy fallback to
/// the user-side `models.json` catalog and the compiled-in builtin
/// catalog.
///
/// Resolution order at lookup time:
///
/// 1. **Pre-resolved** — entries built from
///    `providers.<name>.models.<id>` (the per-(provider, model)
///    override path). Returned as-is.
/// 2. **Lazy synth** — `models.json` (`user_catalog`) ⊕ `builtin`,
///    merged into a fresh `ResolvedModel` with empty
///    `provider_model: ProviderModelOverride::default()`. This is the
///    path that lets users put model metadata in `~/.coco/models.json`
///    without mirroring the entry into every provider's `models` map.
///
/// Returns `None` only when `model_id` appears in NEITHER catalog.
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    /// Pre-resolved (provider, model) entries from each provider's
    /// `cfg.models` map. `BTreeMap` for byte-stable iteration.
    pub resolved: BTreeMap<(String, String), Arc<ResolvedModel>>,
    /// User-supplied `models.json` overlay, indexed by `model_id`.
    /// `base_instructions_file` has already been resolved into
    /// `base_instructions`, so lazy synthesis never does filesystem IO.
    pub user_catalog: BTreeMap<String, PartialModelInfo>,
}

impl ModelRegistry {
    /// Look up a (provider, model_id) pair, returning `Ok(None)` only
    /// when the model is in NEITHER the eager map nor the lazy
    /// (builtin ⊕ user_catalog) overlay. Incomplete entries surface
    /// as `Err(ConfigError::IncompleteModelEntry)` so misconfiguration
    /// is distinguishable from "model not found" — call sites at
    /// startup can fail-fast, runtime call sites convert to a logged
    /// `None` via [`Self::resolve`].
    pub fn try_resolve(
        &self,
        provider: &str,
        model_id: &str,
    ) -> Result<Option<Arc<ResolvedModel>>, ConfigError> {
        if let Some(r) = self
            .resolved
            .get(&(provider.to_string(), model_id.to_string()))
        {
            return Ok(Some(r.clone()));
        }
        // Lazy synth from builtin + user_catalog.
        let builtin = builtin_models_partial();
        let from_builtin = builtin.get(model_id);
        let from_user = self.user_catalog.get(model_id);
        if from_builtin.is_none() && from_user.is_none() {
            return Ok(None);
        }
        let mut acc = from_builtin.cloned().unwrap_or_default();
        if let Some(u) = from_user {
            acc.merge_from(u);
        }
        let info = ModelInfo::from_partial(provider, model_id, acc)?;
        Ok(Some(Arc::new(ResolvedModel {
            info,
            provider_model: ProviderModelOverride::default(),
        })))
    }

    /// Convenience wrapper around [`Self::try_resolve`] for
    /// best-effort runtime call sites. Logs incomplete-entry
    /// configuration errors at WARN and returns `None` so the caller
    /// (e.g. `model_factory::build_api_client`) can fall through.
    /// Startup paths should prefer `try_resolve` directly so a
    /// misconfigured model fails at config-build time.
    pub fn resolve(&self, provider: &str, model_id: &str) -> Option<Arc<ResolvedModel>> {
        match self.try_resolve(provider, model_id) {
            Ok(opt) => opt,
            Err(err) => {
                tracing::warn!(
                    provider,
                    model_id,
                    error = %err,
                    "model registry resolution failed; user-supplied entry is incomplete"
                );
                None
            }
        }
    }
}

/// Build the registry from the resolved provider catalog and the user
/// `models.json` overlay. The compiled-in builtin catalog is merged
/// underneath both.
///
/// `coco_home` is the `~/.coco/` directory; `base_instructions_file`
/// values resolve relative to it. Reading the file is propagated as
/// `ConfigError::BaseInstructionsRead` rather than swallowed.
pub fn build_model_registry(
    providers: &BTreeMap<String, ProviderConfig>,
    user_catalog: &BTreeMap<String, PartialModelInfo>,
    coco_home: &Path,
) -> Result<ModelRegistry, ConfigError> {
    let builtin = builtin_models_partial();
    let user_catalog = normalize_user_catalog(user_catalog, coco_home)?;
    let mut resolved = BTreeMap::new();
    for (provider_name, cfg) in providers {
        for (model_id, entry) in &cfg.models {
            // L0: builtin partial (cached `&'static`; clone is per-pair).
            let mut acc = builtin.get(model_id).cloned().unwrap_or_default();

            // L1: user catalog ~/.coco/models.json.
            if let Some(user_info) = user_catalog.get(model_id) {
                acc.merge_from(user_info);
            }

            // L2: per-(provider, model) entry overrides.
            entry.apply_info_to(&mut acc);

            resolve_base_instructions_file(&mut acc, coco_home)?;

            let info = ModelInfo::from_partial(provider_name, model_id, acc)?;
            resolved.insert(
                (provider_name.clone(), model_id.clone()),
                Arc::new(ResolvedModel {
                    info,
                    provider_model: entry.clone(),
                }),
            );
        }
    }
    Ok(ModelRegistry {
        resolved,
        user_catalog,
    })
}

fn normalize_user_catalog(
    user_catalog: &BTreeMap<String, PartialModelInfo>,
    coco_home: &Path,
) -> Result<BTreeMap<String, PartialModelInfo>, ConfigError> {
    user_catalog
        .iter()
        .map(|(model_id, info)| {
            let mut normalized = info.clone();
            resolve_base_instructions_file(&mut normalized, coco_home)?;
            Ok((model_id.clone(), normalized))
        })
        .collect()
}

fn resolve_base_instructions_file(
    info: &mut PartialModelInfo,
    coco_home: &Path,
) -> Result<(), ConfigError> {
    if let Some(file) = info.base_instructions_file.take() {
        let path = coco_home.join(&file);
        let content =
            std::fs::read_to_string(&path).map_err(|source| ConfigError::BaseInstructionsRead {
                path: path.clone(),
                source,
            })?;
        info.base_instructions = Some(content);
    }
    Ok(())
}

/// Compiled-in builtin model registry — well-known models with known
/// metadata. User catalogue files override these per-key.
///
/// Returned as `&'static` partial form so the registry builder can
/// fold it into the same `merge_from` pipeline as the L1 user catalog
/// without re-cloning per (provider, model) pair.
pub fn builtin_models_partial() -> &'static BTreeMap<String, PartialModelInfo> {
    static BUILTIN: std::sync::OnceLock<BTreeMap<String, PartialModelInfo>> =
        std::sync::OnceLock::new();
    BUILTIN.get_or_init(seed_builtin_models)
}

/// Resolved-form view of the builtin registry. Convenience for
/// callers that want a fully validated `ModelInfo` (e.g. UI listings).
pub fn builtin_models_resolved() -> Vec<ModelInfo> {
    builtin_models_partial()
        .iter()
        .filter_map(|(id, p)| ModelInfo::from_partial("__builtin__", id, p.clone()).ok())
        .collect()
}

fn seed_builtin_models() -> BTreeMap<String, PartialModelInfo> {
    use crate::positive::PositiveTokens;
    use coco_types::ApplyPatchToolType;
    use coco_types::Capability;
    use coco_types::ReasoningEffort;
    use coco_types::ThinkingLevel;
    use coco_types::ToolId;
    use coco_types::ToolName;
    use coco_types::ToolOverrides;

    let mut m = BTreeMap::new();

    let claude_thinking_levels = vec![
        ThinkingLevel::low(),
        ThinkingLevel::medium(),
        ThinkingLevel::high(),
        ThinkingLevel::with_budget(ReasoningEffort::XHigh, 128_000),
    ];

    m.insert(
        "claude-sonnet-4-6".into(),
        PartialModelInfo {
            display_name: Some("Claude Sonnet 4.6".into()),
            context_window: Some(PositiveTokens::new(1_000_000)),
            max_output_tokens: Some(PositiveTokens::new(64_000)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ExtendedThinking,
                Capability::FastMode,
                Capability::PromptCache,
                Capability::Context1m,
                Capability::InterleavedThinking,
                Capability::ContextManagement,
            ]),
            supported_thinking_levels: Some(claude_thinking_levels.clone()),
            default_thinking_level: Some(ReasoningEffort::Medium),
            ..Default::default()
        },
    );

    m.insert(
        "claude-opus-4-7".into(),
        PartialModelInfo {
            display_name: Some("Claude Opus 4.7".into()),
            context_window: Some(PositiveTokens::new(200_000)),
            max_output_tokens: Some(PositiveTokens::new(64_000)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ExtendedThinking,
                Capability::FastMode,
                Capability::PromptCache,
                Capability::InterleavedThinking,
                Capability::ContextManagement,
            ]),
            supported_thinking_levels: Some(claude_thinking_levels),
            default_thinking_level: Some(ReasoningEffort::Medium),
            ..Default::default()
        },
    );

    m.insert(
        "claude-haiku-4-5".into(),
        PartialModelInfo {
            display_name: Some("Claude Haiku 4.5".into()),
            context_window: Some(PositiveTokens::new(200_000)),
            max_output_tokens: Some(PositiveTokens::new(8_192)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::FastMode,
                Capability::PromptCache,
                Capability::ContextManagement,
            ]),
            ..Default::default()
        },
    );

    let gpt5_overrides = ToolOverrides::default()
        .with_extra(ToolId::Builtin(ToolName::ApplyPatch))
        .with_excluded(ToolId::Builtin(ToolName::Edit));

    m.insert(
        "gpt-5-2".into(),
        PartialModelInfo {
            display_name: Some("GPT-5.2".into()),
            context_window: Some(PositiveTokens::new(272_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::StructuredOutput,
                Capability::ReasoningSummaries,
            ]),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            tool_overrides: Some(gpt5_overrides.clone()),
            ..Default::default()
        },
    );

    m.insert(
        "gpt-5-4".into(),
        PartialModelInfo {
            display_name: Some("GPT-5.4".into()),
            base_instructions: builtin_base_instructions("gpt-5-4").map(str::to_string),
            context_window: Some(PositiveTokens::new(272_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ReasoningSummaries,
                Capability::ParallelToolCalls,
            ]),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            tool_overrides: Some(gpt5_overrides.clone()),
            ..Default::default()
        },
    );

    m.insert(
        "gpt-5-5".into(),
        PartialModelInfo {
            display_name: Some("GPT-5.5".into()),
            base_instructions: builtin_base_instructions("gpt-5-5").map(str::to_string),
            context_window: Some(PositiveTokens::new(272_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ExtendedThinking,
                Capability::ReasoningSummaries,
                Capability::ParallelToolCalls,
            ]),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            tool_overrides: Some(gpt5_overrides.clone()),
            ..Default::default()
        },
    );

    m.insert(
        "gpt-5-3-codex".into(),
        PartialModelInfo {
            display_name: Some("GPT-5.3 Codex".into()),
            base_instructions: builtin_base_instructions("gpt-5-3-codex").map(str::to_string),
            context_window: Some(PositiveTokens::new(272_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ReasoningSummaries,
                Capability::ParallelToolCalls,
            ]),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            tool_overrides: Some(gpt5_overrides),
            ..Default::default()
        },
    );

    m.insert(
        "gemini-2.5-pro".into(),
        PartialModelInfo {
            display_name: Some("Gemini 2.5 Pro".into()),
            base_instructions: builtin_base_instructions("gemini-2.5-pro").map(str::to_string),
            context_window: Some(PositiveTokens::new(1_000_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
                Capability::ExtendedThinking,
            ]),
            ..Default::default()
        },
    );

    m.insert(
        "gemini-2.5-flash".into(),
        PartialModelInfo {
            display_name: Some("Gemini 2.5 Flash".into()),
            base_instructions: builtin_base_instructions("gemini-2.5-flash").map(str::to_string),
            context_window: Some(PositiveTokens::new(1_000_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
                Capability::Vision,
            ]),
            ..Default::default()
        },
    );

    m.insert(
        "deepseek-v4-flash".into(),
        PartialModelInfo {
            display_name: Some("DeepSeek V4 Flash".into()),
            base_instructions: builtin_base_instructions("deepseek-v4-flash").map(str::to_string),
            context_window: Some(PositiveTokens::new(1_000_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    m.insert(
        "deepseek-v4-pro".into(),
        PartialModelInfo {
            display_name: Some("DeepSeek V4 Pro".into()),
            base_instructions: builtin_base_instructions("deepseek-v4-pro").map(str::to_string),
            context_window: Some(PositiveTokens::new(1_000_000)),
            max_output_tokens: Some(PositiveTokens::new(12_288)),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    m
}

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
