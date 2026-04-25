//! Multi-provider `ModelSpec → Arc<dyn LanguageModelV4>` factory.
//!
//! The inference layer (`services/inference`) is deliberately
//! provider-agnostic — only the `vercel-ai-provider` trait + the
//! high-level `vercel-ai` SDK are in scope there. Concrete provider
//! construction has to happen in a layer that's allowed to depend on
//! each `vercel-ai-{anthropic,openai,google,bytedance,openai-compatible}`
//! crate, which is the CLI.
//!
//! TS parity: provider dispatch lives in `src/services/api/` + the
//! individual provider SDKs; all plumbed through a single entry that
//! picks by `ProviderApi`.
//!
//! Usage:
//! ```ignore
//! use crate::model_factory::build_language_model_from_spec;
//! let fast_spec = model_roles.get(ModelRole::Fast).cloned()?;
//! let fast_model = build_language_model_from_spec(&fast_spec)?;
//! let fast_client = Arc::new(ApiClient::new(fast_model, RetryConfig::default()));
//! ```

use std::sync::Arc;

use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::ProviderV4;

/// Build a `LanguageModelV4` for the given spec. Fails with a clear
/// error if the provider isn't compiled in (for the two "Custom" /
/// unknown cases).
pub fn build_language_model_from_spec(
    spec: &ModelSpec,
) -> anyhow::Result<Arc<dyn LanguageModelV4>> {
    match spec.api {
        ProviderApi::Anthropic => {
            let provider = vercel_ai_anthropic::anthropic();
            provider
                .language_model(&spec.model_id)
                .map_err(|e| anyhow::anyhow!("anthropic: {e}"))
        }
        ProviderApi::Openai => {
            let provider = vercel_ai_openai::openai();
            provider
                .language_model(&spec.model_id)
                .map_err(|e| anyhow::anyhow!("openai: {e}"))
        }
        ProviderApi::Gemini => {
            let provider = vercel_ai_google::google();
            provider
                .language_model(&spec.model_id)
                .map_err(|e| anyhow::anyhow!("google: {e}"))
        }
        // Volcengine / Zai / OpenaiCompat all need provider-specific
        // base URL / auth setup that `ModelSpec` doesn't carry today.
        // Callers should construct the provider explicitly until we
        // extend `ModelSpec` (or introduce a typed `ResolvedProvider`).
        ProviderApi::Volcengine | ProviderApi::Zai | ProviderApi::OpenaiCompat => {
            Err(anyhow::anyhow!(
                "ModelSpec api={:?} is not wired into \
                 build_language_model_from_spec yet; construct the provider \
                 explicitly",
                spec.api
            ))
        }
    }
}

/// Build an `ApiClient` from a `ModelSpec` with the given retry
/// config. Shared across primary + fallback client construction so
/// all slots in a session use the same retry policy.
pub fn build_api_client(spec: &ModelSpec, retry: RetryConfig) -> anyhow::Result<Arc<ApiClient>> {
    let model = build_language_model_from_spec(spec)?;
    Ok(Arc::new(ApiClient::new(model, retry)))
}

/// Resolve the fallback chain for a role and build one `ApiClient`
/// per tier. Returns an empty vec when the role has no configured
/// fallbacks.
///
/// Fail-fast on any tier that can't construct: silently dropping a
/// fallback would only surface under outage, which is exactly when
/// the user can least afford to discover it.
pub fn build_fallback_clients_for_role(
    runtime: &coco_config::RuntimeConfig,
    role: ModelRole,
    retry: RetryConfig,
) -> anyhow::Result<Vec<Arc<ApiClient>>> {
    runtime
        .model_roles
        .fallbacks(role)
        .iter()
        .map(|spec| build_api_client(spec, retry.clone()))
        .collect()
}

#[cfg(test)]
#[path = "model_factory.test.rs"]
mod tests;
