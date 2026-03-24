//! Unified model hub for model acquisition and caching.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::LanguageModel;
use crate::Provider;
use cocode_config::Config;
use cocode_protocol::PromptCacheConfig;
use cocode_protocol::ProviderApi;
use cocode_protocol::execution::AgentKind;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::execution::InferenceContext;
use cocode_protocol::model::ModelInfo;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use cocode_protocol::model::RoleSelection;
use cocode_protocol::model::RoleSelections;
use cocode_protocol::thinking::ThinkingLevel;
use tracing::debug;
use tracing::info;
use uuid::Uuid;

use crate::error::api_error::InvalidRequestSnafu;
use crate::error::api_error::SdkSnafu;
use crate::provider_factory;

/// Resolved model entry: (model, model_info, provider_api, provider_options).
type ResolvedModelEntry = (
    Arc<dyn LanguageModel>,
    ModelInfo,
    ProviderApi,
    HashMap<String, serde_json::Value>,
);

// ============================================================================
// Identity Resolution (Standalone Function)
// ============================================================================

/// Resolve an ExecutionIdentity to a ModelSpec and optional RoleSelection.
pub fn resolve_identity(
    identity: &ExecutionIdentity,
    selections: &RoleSelections,
    parent_spec: Option<&ModelSpec>,
) -> crate::error::Result<(ModelSpec, Option<RoleSelection>)> {
    match identity {
        ExecutionIdentity::Role(role) => {
            let selection = selections
                .get_or_main(*role)
                .ok_or_else(|| {
                    InvalidRequestSnafu {
                        message: format!("No model configured for role:{role}"),
                    }
                    .build()
                })?
                .clone();

            Ok((selection.model.clone(), Some(selection)))
        }
        ExecutionIdentity::Spec(spec) => Ok((spec.clone(), None)),
        ExecutionIdentity::Inherit => {
            let spec = parent_spec
                .ok_or_else(|| {
                    InvalidRequestSnafu {
                        message: "Inherit identity requires parent_spec but none was provided"
                            .to_string(),
                    }
                    .build()
                })?
                .clone();
            Ok((spec, None))
        }
    }
}

// ============================================================================
// Cached Types
// ============================================================================

struct CachedProvider {
    provider: Arc<dyn Provider>,
    api: ProviderApi,
}

struct CachedModel {
    model: Arc<dyn LanguageModel>,
    model_info: ModelInfo,
    api: ProviderApi,
    model_options: HashMap<String, serde_json::Value>,
}

// ============================================================================
// ModelHub
// ============================================================================

/// Unified model hub for model acquisition and caching.
pub struct ModelHub {
    config: Arc<Config>,
    providers: RwLock<HashMap<String, CachedProvider>>,
    models: RwLock<HashMap<ModelSpec, CachedModel>>,
}

impl ModelHub {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            providers: RwLock::new(HashMap::new()),
            models: RwLock::new(HashMap::new()),
        }
    }

    /// Build a complete inference context from a resolved model spec.
    pub fn build_context(
        &self,
        spec: &ModelSpec,
        session_id: &str,
        turn_number: i32,
        agent_kind: AgentKind,
        thinking_level: Option<ThinkingLevel>,
        original_identity: ExecutionIdentity,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn LanguageModel>)> {
        let (model, model_info, _api, model_options) = self.get_or_create_model(spec)?;

        let call_id = Uuid::new_v4().to_string();
        let mut ctx = InferenceContext::new(
            call_id,
            session_id,
            turn_number,
            spec.clone(),
            model_info.clone(),
            agent_kind,
            original_identity,
        );

        // Merge shared options + per-provider model_options
        {
            let has_shared = model_info.options.as_ref().is_some_and(|o| !o.is_empty());
            let has_override = !model_options.is_empty();
            if has_shared || has_override {
                let mut opts = model_info.options.unwrap_or_default();
                for (k, v) in &model_options {
                    opts.insert(k.clone(), v.clone());
                }
                ctx = ctx.with_request_options(opts);
            }
        }

        if let Some(level) = thinking_level {
            ctx = ctx.with_thinking_level(level);
        }

        // Wire interceptor names from provider config
        if let Some(provider_info) = self.config.provider(&spec.provider)
            && !provider_info.interceptors.is_empty()
        {
            ctx = ctx.with_interceptor_names(provider_info.interceptors.clone());
        }

        // Enable prompt caching for Anthropic providers
        if spec.api == ProviderApi::Anthropic {
            ctx = ctx.with_prompt_cache_config(PromptCacheConfig::default());
        }

        debug!(
            call_id = %ctx.call_id,
            model = %ctx.model_spec,
            thinking = ?ctx.thinking_level,
            "Built inference context"
        );

        Ok((ctx, model))
    }

    /// Prepare inference context with selections passed as parameter.
    pub fn prepare_inference_with_selections(
        &self,
        identity: &ExecutionIdentity,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
        agent_kind: AgentKind,
        parent_spec: Option<&ModelSpec>,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn LanguageModel>)> {
        let (spec, selection) = resolve_identity(identity, selections, parent_spec)?;
        let thinking_level = selection.and_then(|s| s.thinking_level);

        self.build_context(
            &spec,
            session_id,
            turn_number,
            agent_kind,
            thinking_level,
            identity.clone(),
        )
    }

    pub fn prepare_main_with_selections(
        &self,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn LanguageModel>)> {
        self.prepare_inference_with_selections(
            &ExecutionIdentity::main(),
            selections,
            session_id,
            turn_number,
            AgentKind::Main,
            None,
        )
    }

    pub fn prepare_compact_with_selections(
        &self,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn LanguageModel>)> {
        self.prepare_inference_with_selections(
            &ExecutionIdentity::compact(),
            selections,
            session_id,
            turn_number,
            AgentKind::Compaction,
            None,
        )
    }

    // ========================================================================
    // Model Access (Direct - Role-Agnostic)
    // ========================================================================

    pub fn get_model(
        &self,
        spec: &ModelSpec,
    ) -> crate::error::Result<(Arc<dyn LanguageModel>, ProviderApi)> {
        self.get_or_create_model(spec).map(|(m, _, pt, _)| (m, pt))
    }

    pub fn get_model_with_info(
        &self,
        spec: &ModelSpec,
    ) -> crate::error::Result<(Arc<dyn LanguageModel>, ModelInfo, ProviderApi)> {
        self.get_or_create_model(spec)
            .map(|(m, info, pt, _)| (m, info, pt))
    }

    pub fn get_model_for_role_with_selections(
        &self,
        role: ModelRole,
        selections: &RoleSelections,
    ) -> crate::error::Result<(Arc<dyn LanguageModel>, ProviderApi)> {
        let selection = selections.get_or_main(role).ok_or_else(|| {
            InvalidRequestSnafu {
                message: format!("No model configured for role:{role}"),
            }
            .build()
        })?;

        let spec = &selection.model;
        self.get_or_create_model(spec).map(|(m, _, pt, _)| (m, pt))
    }

    // ========================================================================
    // Cache Management
    // ========================================================================

    pub fn invalidate_model(&self, spec: &ModelSpec) {
        let mut cache = self
            .models
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.remove(spec).is_some() {
            debug!(
                provider = %spec.provider,
                model = %spec.slug,
                "Invalidated cached model"
            );
        }
    }

    pub fn invalidate_provider(&self, provider_name: &str) {
        {
            let mut cache = self
                .providers
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.remove(provider_name).is_some() {
                debug!(provider = %provider_name, "Invalidated cached provider");
            }
        }

        let mut cache = self
            .models
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let to_remove: Vec<ModelSpec> = cache
            .keys()
            .filter(|spec| spec.provider == provider_name)
            .cloned()
            .collect();

        for spec in to_remove {
            cache.remove(&spec);
            debug!(
                provider = %spec.provider,
                model = %spec.slug,
                "Invalidated cached model (provider invalidation)"
            );
        }
    }

    pub fn invalidate_all(&self) {
        self.providers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.models
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        debug!("Invalidated all cached providers and models");
    }

    pub fn provider_cache_size(&self) -> usize {
        self.providers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn model_cache_size(&self) -> usize {
        self.models
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    // ========================================================================
    // Private Helpers
    // ========================================================================

    fn get_or_create_model(&self, spec: &ModelSpec) -> crate::error::Result<ResolvedModelEntry> {
        // Phase 1: Check model cache
        {
            let cache = self.models.read().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;
            if let Some(cached) = cache.get(spec) {
                debug!(
                    provider = %spec.provider,
                    model = %spec.slug,
                    "Model cache hit"
                );
                return Ok((
                    cached.model.clone(),
                    cached.model_info.clone(),
                    cached.api,
                    cached.model_options.clone(),
                ));
            }
        }

        // Phase 2: Get or create provider
        let (provider, provider_api) = self.get_or_create_provider(&spec.provider)?;

        // Phase 3: Resolve model info and alias
        let provider_model = self
            .config
            .resolve_provider_model(&spec.provider, &spec.slug)
            .ok_or_else(|| {
                InvalidRequestSnafu {
                    message: format!(
                        "Model '{}' not found in provider '{}'",
                        spec.slug, spec.provider
                    ),
                }
                .build()
            })?;

        let model_info = provider_model.model_info.clone();
        let model_options = provider_model.model_options.clone();
        let api_model_name = provider_model.api_model_name();

        info!(
            provider = %spec.provider,
            model = %spec.slug,
            api_model = %api_model_name,
            "Creating model"
        );

        let model: Arc<dyn LanguageModel> =
            provider.language_model(api_model_name).map_err(|e| {
                SdkSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        // Phase 4: Double-check and store
        {
            let mut cache = self.models.write().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;

            if let Some(cached) = cache.get(spec) {
                debug!(
                    provider = %spec.provider,
                    model = %spec.slug,
                    "Model created by another thread, using existing"
                );
                return Ok((
                    cached.model.clone(),
                    cached.model_info.clone(),
                    cached.api,
                    cached.model_options.clone(),
                ));
            }

            cache.insert(
                spec.clone(),
                CachedModel {
                    model: model.clone(),
                    model_info: model_info.clone(),
                    api: provider_api,
                    model_options: model_options.clone(),
                },
            );
        }

        Ok((model, model_info, provider_api, model_options))
    }

    fn get_or_create_provider(
        &self,
        provider_name: &str,
    ) -> crate::error::Result<(Arc<dyn Provider>, ProviderApi)> {
        // Phase 1: Check provider cache
        {
            let cache = self.providers.read().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;
            if let Some(cached) = cache.get(provider_name) {
                debug!(provider = %provider_name, "Provider cache hit");
                return Ok((cached.provider.clone(), cached.api));
            }
        }

        // Phase 2: Resolve provider info and create
        let provider_info = self.config.provider(provider_name).ok_or_else(|| {
            InvalidRequestSnafu {
                message: format!("Provider '{provider_name}' not found in config"),
            }
            .build()
        })?;

        info!(provider = %provider_name, "Creating provider");
        let provider = provider_factory::create_provider(provider_info)?;
        let api = provider_info.api;

        // Phase 3: Double-check and store
        {
            let mut cache = self.providers.write().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;

            if let Some(cached) = cache.get(provider_name) {
                debug!(
                    provider = %provider_name,
                    "Provider created by another thread, using existing"
                );
                return Ok((cached.provider.clone(), cached.api));
            }

            cache.insert(
                provider_name.to_string(),
                CachedProvider {
                    provider: provider.clone(),
                    api,
                },
            );
        }

        Ok((provider, api))
    }
}

impl std::fmt::Debug for ModelHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelHub")
            .field("provider_cache_size", &self.provider_cache_size())
            .field("model_cache_size", &self.model_cache_size())
            .finish()
    }
}

#[cfg(test)]
#[path = "model_hub.test.rs"]
mod tests;
