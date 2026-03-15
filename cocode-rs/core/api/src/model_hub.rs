//! Unified model hub for model acquisition and caching.
//!
//! `ModelHub` is the central service for acquiring model instances and building
//! inference contexts. It is **stateless** regarding role selections - selections
//! are passed as parameters to enable proper session isolation.
//!
//! # Key Features
//!
//! - **Role-agnostic**: ModelHub does NOT know about roles; it only resolves ModelSpec → Model
//! - **Stateless for selections**: RoleSelections are passed as parameters (owned by Session)
//! - **Provider and model caching**: Reuses expensive HTTP clients and model instances
//! - **Full context preparation**: `prepare_inference_with_selections()` returns complete `InferenceContext`
//!
//! # Architecture
//!
//! ```text
//! Session (OWNS selections)
//!     │
//!     ├─► resolve_identity() → (ModelSpec, ThinkingLevel)
//!     │       Uses session.selections
//!     │
//!     └─► ModelHub (ROLE-AGNOSTIC)
//!             │
//!             └─► get_model(spec) → (Arc<dyn Model>, ModelInfo)
//!                 build_context(spec, ...) → (InferenceContext, Model)
//! ```
//!
//! # Example
//!
//! ```ignore
//! use cocode_api::ModelHub;
//! use cocode_protocol::execution::{ExecutionIdentity, AgentKind, resolve_identity};
//! use cocode_protocol::model::ModelRole;
//!
//! let hub = ModelHub::new(config);
//!
//! // Step 1: Resolve identity using session's selections
//! let (spec, thinking_level) = resolve_identity(
//!     &ExecutionIdentity::main(),
//!     &session.selections,
//!     None,  // no parent spec
//! )?;
//!
//! // Step 2: Get model from hub (role-agnostic)
//! let (ctx, model) = hub.build_context(
//!     &spec,
//!     "session-123",
//!     1,
//!     AgentKind::Main,
//!     thinking_level,
//! )?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use cocode_config::Config;
use cocode_protocol::ProviderType;
use cocode_protocol::execution::AgentKind;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::execution::InferenceContext;
use cocode_protocol::model::ModelInfo;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use cocode_protocol::model::RoleSelection;
use cocode_protocol::model::RoleSelections;
use cocode_protocol::thinking::ThinkingLevel;
use hyper_sdk::Model;
use hyper_sdk::Provider;
use tracing::debug;
use tracing::info;
use uuid::Uuid;

use crate::error::ApiError;
use crate::error::api_error::InvalidRequestSnafu;
use crate::error::api_error::SdkSnafu;
use crate::provider_factory;

// ============================================================================
// Identity Resolution (Standalone Function)
// ============================================================================

/// Resolve an ExecutionIdentity to a ModelSpec and optional RoleSelection.
///
/// This is the **public** function for identity resolution. It takes selections
/// as a parameter (not from internal state), enabling proper session isolation.
///
/// # Arguments
///
/// * `identity` - How to resolve the model (Role, Spec, or Inherit)
/// * `selections` - Role selections (owned by Session, passed as parameter)
/// * `parent_spec` - Parent model spec for Inherit identity
///
/// # Returns
///
/// A tuple of (ModelSpec, Option<RoleSelection>) on success:
/// - For Role: returns (spec from selection, full selection with thinking level)
/// - For Spec: returns (the spec directly, None)
/// - For Inherit: returns (parent spec, None)
///
/// # Example
///
/// ```ignore
/// use cocode_api::resolve_identity;
/// use cocode_protocol::execution::ExecutionIdentity;
///
/// // Resolve main role to model spec
/// let (spec, selection) = resolve_identity(
///     &ExecutionIdentity::main(),
///     &session.selections,
///     None,
/// )?;
///
/// // Get thinking level from selection
/// let thinking_level = selection.and_then(|s| s.thinking_level);
/// ```
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
        ExecutionIdentity::Spec(spec) => {
            // Direct spec: no selection override
            Ok((spec.clone(), None))
        }
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

/// A cached provider instance.
struct CachedProvider {
    provider: Arc<dyn Provider>,
    provider_type: ProviderType,
}

/// A cached model instance with resolved info.
struct CachedModel {
    model: Arc<dyn Model>,
    model_info: ModelInfo,
    provider_type: ProviderType,
    /// Per-provider model options (deferred merge at build_context time).
    model_options: HashMap<String, serde_json::Value>,
}

// ============================================================================
// ModelHub
// ============================================================================

/// Unified model hub for model acquisition and caching.
///
/// `ModelHub` is a role-agnostic service that:
/// - Acquires model instances from providers
/// - Caches providers and models for reuse
/// - Builds `InferenceContext` for request building
///
/// # Stateless Design
///
/// ModelHub does NOT own or manage `RoleSelections`. Instead:
/// - Session owns its `RoleSelections`
/// - Callers resolve `ExecutionIdentity → ModelSpec` using `resolve_identity()`
/// - Then call ModelHub with the resolved `ModelSpec`
///
/// This design enables proper session isolation - subagents receive cloned
/// selections at spawn time and are unaffected by parent model changes.
///
/// # Thread Safety
///
/// Uses `RwLock` for caches to allow concurrent reads with exclusive writes.
/// Model/provider creation happens outside locks to avoid blocking.
pub struct ModelHub {
    config: Arc<Config>,
    /// Cached providers keyed by provider name.
    providers: RwLock<HashMap<String, CachedProvider>>,
    /// Cached models keyed by ModelSpec.
    models: RwLock<HashMap<ModelSpec, CachedModel>>,
}

impl ModelHub {
    /// Create a new model hub.
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            providers: RwLock::new(HashMap::new()),
            models: RwLock::new(HashMap::new()),
        }
    }

    // ========================================================================
    // Core API: Prepare Inference Context
    // ========================================================================

    /// Build a complete inference context from a resolved model spec.
    ///
    /// This is the **recommended** entry point. The caller is responsible for:
    /// 1. Resolving `ExecutionIdentity → ModelSpec` using `resolve_identity()`
    /// 2. Calling this method with the resolved spec
    ///
    /// # Arguments
    ///
    /// * `spec` - Already-resolved model specification
    /// * `session_id` - Session ID for correlation
    /// * `turn_number` - Turn number within the session
    /// * `agent_kind` - Type of agent making the request
    /// * `thinking_level` - Optional thinking level override
    /// * `original_identity` - The original identity (for telemetry)
    ///
    /// # Returns
    ///
    /// A tuple of (InferenceContext, Model) on success.
    pub fn build_context(
        &self,
        spec: &ModelSpec,
        session_id: &str,
        turn_number: i32,
        agent_kind: AgentKind,
        thinking_level: Option<ThinkingLevel>,
        original_identity: ExecutionIdentity,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn Model>)> {
        // Get or create model
        let (model, model_info, _provider_type, model_options) = self.get_or_create_model(spec)?;

        // Build inference context
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

        // Merge shared options (info.options) + per-provider model_options at call time.
        // Per-provider model_options take precedence over shared options.
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

        // Apply thinking level if provided
        if let Some(level) = thinking_level {
            ctx = ctx.with_thinking_level(level);
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
    ///
    /// This is the main entry point that:
    /// 1. Resolves the `ExecutionIdentity` to a `ModelSpec` using provided selections
    /// 2. Gets or creates the model instance
    /// 3. Builds `InferenceContext` ready for `RequestBuilder`
    ///
    /// # Arguments
    ///
    /// * `identity` - How to resolve the model (Role, Spec, or Inherit)
    /// * `selections` - Role selections (owned by Session, passed as parameter)
    /// * `session_id` - Session ID for correlation
    /// * `turn_number` - Turn number within the session
    /// * `agent_kind` - Type of agent making the request
    /// * `parent_spec` - Parent model spec for Inherit identity
    pub fn prepare_inference_with_selections(
        &self,
        identity: &ExecutionIdentity,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
        agent_kind: AgentKind,
        parent_spec: Option<&ModelSpec>,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn Model>)> {
        // Step 1: Resolve identity to spec and selection
        let (spec, selection) = resolve_identity(identity, selections, parent_spec)?;

        // Step 2: Get thinking level from selection
        let thinking_level = selection.and_then(|s| s.thinking_level);

        // Step 3: Build context with resolved spec
        self.build_context(
            &spec,
            session_id,
            turn_number,
            agent_kind,
            thinking_level,
            identity.clone(),
        )
    }

    /// Convenience: prepare context for main conversation with selections.
    pub fn prepare_main_with_selections(
        &self,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn Model>)> {
        self.prepare_inference_with_selections(
            &ExecutionIdentity::main(),
            selections,
            session_id,
            turn_number,
            AgentKind::Main,
            None,
        )
    }

    /// Convenience: prepare context for compaction with selections.
    pub fn prepare_compact_with_selections(
        &self,
        selections: &RoleSelections,
        session_id: &str,
        turn_number: i32,
    ) -> crate::error::Result<(InferenceContext, Arc<dyn Model>)> {
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

    /// Get model by explicit ModelSpec.
    ///
    /// This is the core model acquisition method - completely role-agnostic.
    pub fn get_model(
        &self,
        spec: &ModelSpec,
    ) -> crate::error::Result<(Arc<dyn Model>, ProviderType)> {
        self.get_or_create_model(spec).map(|(m, _, pt, _)| (m, pt))
    }

    /// Get model and info by explicit ModelSpec.
    ///
    /// Returns the model instance, model info, and provider type.
    pub fn get_model_with_info(
        &self,
        spec: &ModelSpec,
    ) -> crate::error::Result<(Arc<dyn Model>, ModelInfo, ProviderType)> {
        self.get_or_create_model(spec)
            .map(|(m, info, pt, _)| (m, info, pt))
    }

    /// Get model for a role using provided selections.
    ///
    /// Use `prepare_inference_with_selections()` when you need the full `InferenceContext`.
    /// This method is for cases where you only need the model instance.
    pub fn get_model_for_role_with_selections(
        &self,
        role: ModelRole,
        selections: &RoleSelections,
    ) -> crate::error::Result<(Arc<dyn Model>, ProviderType)> {
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

    /// Invalidate cached model for a specific spec.
    pub fn invalidate_model(&self, spec: &ModelSpec) {
        if let Ok(mut cache) = self.models.write()
            && cache.remove(spec).is_some()
        {
            debug!(
                provider = %spec.provider,
                model = %spec.slug,
                "Invalidated cached model"
            );
        }
    }

    /// Invalidate cached provider (and all its models).
    pub fn invalidate_provider(&self, provider_name: &str) {
        // Remove provider
        if let Ok(mut cache) = self.providers.write()
            && cache.remove(provider_name).is_some()
        {
            debug!(provider = %provider_name, "Invalidated cached provider");
        }

        // Remove all models for this provider
        if let Ok(mut cache) = self.models.write() {
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
    }

    /// Invalidate all caches.
    pub fn invalidate_all(&self) {
        if let Ok(mut cache) = self.providers.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.models.write() {
            cache.clear();
        }
        debug!("Invalidated all cached providers and models");
    }

    /// Get the number of cached providers.
    pub fn provider_cache_size(&self) -> usize {
        self.providers.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Get the number of cached models.
    pub fn model_cache_size(&self) -> usize {
        self.models.read().map(|c| c.len()).unwrap_or(0)
    }

    // ========================================================================
    // Private Helpers
    // ========================================================================

    /// Get or create a model instance, returning model, info, provider type, and model_options.
    fn get_or_create_model(
        &self,
        spec: &ModelSpec,
    ) -> crate::error::Result<(
        Arc<dyn Model>,
        ModelInfo,
        ProviderType,
        HashMap<String, serde_json::Value>,
    )> {
        // Phase 1: Check model cache (read lock)
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
                    cached.provider_type,
                    cached.model_options.clone(),
                ));
            }
        }

        // Phase 2: Get or create provider
        let (provider, provider_type) = self.get_or_create_provider(&spec.provider)?;

        // Phase 3: Resolve model info and alias.
        // Note: Phase 2 already did a full resolve_provider() inside get_or_create_provider()
        // (genuinely needed to build the HTTP client). This phase only does lightweight lookups.
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

        // ProviderCreation: inner error is already ApiError, propagate directly
        let model: Arc<dyn Model> = provider.model(api_model_name).map_err(ApiError::from)?;

        // Phase 4: Double-check and store in model cache (write lock)
        {
            let mut cache = self.models.write().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;

            // Another thread might have created it
            if let Some(cached) = cache.get(spec) {
                debug!(
                    provider = %spec.provider,
                    model = %spec.slug,
                    "Model created by another thread, using existing"
                );
                return Ok((
                    cached.model.clone(),
                    cached.model_info.clone(),
                    cached.provider_type,
                    cached.model_options.clone(),
                ));
            }

            cache.insert(
                spec.clone(),
                CachedModel {
                    model: model.clone(),
                    model_info: model_info.clone(),
                    provider_type,
                    model_options: model_options.clone(),
                },
            );
        }

        Ok((model, model_info, provider_type, model_options))
    }

    /// Get or create a provider instance.
    fn get_or_create_provider(
        &self,
        provider_name: &str,
    ) -> crate::error::Result<(Arc<dyn Provider>, ProviderType)> {
        // Phase 1: Check provider cache (read lock)
        {
            let cache = self.providers.read().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;
            if let Some(cached) = cache.get(provider_name) {
                debug!(provider = %provider_name, "Provider cache hit");
                return Ok((cached.provider.clone(), cached.provider_type));
            }
        }

        // Phase 2: Resolve provider info and create provider
        let provider_info = self.config.provider(provider_name).ok_or_else(|| {
            InvalidRequestSnafu {
                message: format!("Provider '{provider_name}' not found in config"),
            }
            .build()
        })?;

        info!(provider = %provider_name, "Creating provider");
        // ProviderCreation: inner error is already ApiError, propagate directly
        let provider = provider_factory::create_provider(provider_info)?;
        let provider_type = provider_info.provider_type;

        // Phase 3: Double-check and store in cache (write lock)
        {
            let mut cache = self.providers.write().map_err(|_| {
                SdkSnafu {
                    message: "Internal lock poisoned",
                }
                .build()
            })?;

            // Another thread might have created it
            if let Some(cached) = cache.get(provider_name) {
                debug!(
                    provider = %provider_name,
                    "Provider created by another thread, using existing"
                );
                return Ok((cached.provider.clone(), cached.provider_type));
            }

            cache.insert(
                provider_name.to_string(),
                CachedProvider {
                    provider: provider.clone(),
                    provider_type,
                },
            );
        }

        Ok((provider, provider_type))
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[path = "model_hub.test.rs"]
mod tests;
