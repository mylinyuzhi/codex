//! Multi-provider `RuntimeConfig + ModelSpec → Arc<dyn LanguageModel>` factory.
//!
//! The inference layer (`services/inference`) is deliberately
//! provider-agnostic — only the `vercel-ai-provider` trait + the
//! high-level `vercel-ai` SDK are in scope there. Concrete provider
//! construction has to happen in a layer that's allowed to depend on
//! each `vercel-ai-{anthropic,openai,google,openai-compatible}` crate,
//! which is the CLI.
//!
//! `build_language_model_from_runtime` is the single binding point
//! between `RuntimeConfig` (Layer 1) and the `vercel-ai-*` crates
//! (Layer 3) — see `multi-provider-plan.md` §6. It threads the typed
//! `ProviderClientOptions` (`headers`, `auth_token`, `organization_id`,
//! `project_id`, `include_usage`, `full_url`,
//! `supports_structured_outputs`) into each provider's
//! `*ProviderSettings`. The match is exhaustive across all six
//! `ProviderApi` variants — adding a new variant is a compile error
//! here.
//!
//! `build_api_client` produces an `Arc<ApiClient>` whose
//! `ProviderClientFingerprint` is computed from the resolved
//! `ProviderConfig` via `ProviderClientFingerprint::compute`. The
//! turn-boundary coherence check (multi-provider-plan §11.1) compares
//! against this fingerprint after hot-reload.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use coco_config::ModelInfo;
use coco_config::ProviderClientOptions;
use coco_config::ProviderConfig;
use coco_config::RuntimeConfig;
use coco_types::Capability;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use coco_types::WireApi;
use tracing::warn;

use crate::ApiClient;
use crate::InferenceError;
use crate::LanguageModel;
use crate::Provider;
use crate::ProviderClientFingerprint;
use crate::RetryConfig;

/// Surface a `tracing::warn` when a `ProviderClientOptions` field is
/// set to a non-default value but the chosen provider doesn't consume
/// it. Helps users catch typos like setting `auth_token` on an OpenAI
/// instance (which expects `api_key`) or `organization_id` on an
/// Anthropic instance.
fn warn_unused_client_options(provider_name: &str, api: ProviderApi, opts: &ProviderClientOptions) {
    let provider_label = format!("{provider_name} (api={api:?})");

    // auth_token: Anthropic only.
    if opts.auth_token.is_some() && api != ProviderApi::Anthropic {
        warn!(
            provider = %provider_label,
            "client_options.auth_token is Anthropic-specific; ignored on this provider"
        );
    }
    // organization_id / project_id: OpenAI direct only.
    if (opts.organization_id.is_some() || opts.project_id.is_some()) && api != ProviderApi::Openai {
        warn!(
            provider = %provider_label,
            "client_options.organization_id / project_id are OpenAI-specific; ignored on this provider"
        );
    }
    // include_usage / supports_structured_outputs: OpenAI-compat family only.
    let is_compat = matches!(
        api,
        ProviderApi::OpenaiCompat | ProviderApi::Volcengine | ProviderApi::Zai
    );
    if (opts.include_usage.is_some() || opts.supports_structured_outputs) && !is_compat {
        warn!(
            provider = %provider_label,
            "client_options.include_usage / supports_structured_outputs are OpenAI-compat-specific; ignored on this provider"
        );
    }
    // full_url: not honored by Google.
    if opts.full_url && api == ProviderApi::Gemini {
        warn!(
            provider = %provider_label,
            "client_options.full_url is not honored by the Google SDK; ignored"
        );
    }
}

/// Build an `Arc<dyn LanguageModel>` for the (provider, model) pair
/// referenced by `spec`. All six `ProviderApi` variants are wired:
/// `Anthropic` / `Openai` / `Gemini` go through their direct SDKs;
/// `Volcengine` / `Zai` / `OpenaiCompat` route through
/// `vercel-ai-openai-compatible` with the runtime instance name as the
/// SDK's `provider_id` so `model.provider()` round-trips back to
/// `ProviderConfig.name` (closes §7.2). Per-model
/// `info.timeout_secs` (when set) overrides the provider-level
/// timeout for the HTTP client.
pub fn build_language_model_from_runtime(
    runtime: &RuntimeConfig,
    spec: &ModelSpec,
) -> Result<Arc<dyn LanguageModel>, InferenceError> {
    let provider_cfg = runtime.providers.get(&spec.provider).ok_or_else(|| {
        crate::errors::UnknownProviderSnafu {
            provider: spec.provider.clone(),
        }
        .build()
    })?;
    let api_model_name = resolve_api_model_name(provider_cfg, &spec.model_id);
    warn_unused_client_options(&provider_cfg.name, spec.api, &provider_cfg.client_options);
    let model_info = runtime
        .model_registry
        .resolve(&spec.provider, &spec.model_id)
        .map(|r| r.info.clone());
    let timeout_secs = effective_timeout_secs(provider_cfg, model_info.as_ref());

    match spec.api {
        ProviderApi::Anthropic => build_anthropic(
            runtime,
            provider_cfg,
            &api_model_name,
            timeout_secs,
            model_info.as_ref(),
        ),
        ProviderApi::Openai => build_openai(provider_cfg, &api_model_name, timeout_secs),
        ProviderApi::Gemini => build_google(provider_cfg, &api_model_name),
        ProviderApi::Volcengine | ProviderApi::Zai | ProviderApi::OpenaiCompat => {
            build_openai_compat(provider_cfg, &api_model_name, timeout_secs)
        }
    }
}

/// Build an `ApiClient` from a `RuntimeConfig + ModelSpec`.
///
/// - The `ProviderClientFingerprint` is computed from the resolved
///   `ProviderConfig` so the turn-boundary coherence check
///   (multi-provider-plan §11.1) detects a stale cached client
///   after hot-reload.
/// - The resolved `ModelInfo` (via `runtime.model_registry.resolve`)
///   is threaded into the client so `query` / `query_stream` can
///   route through `build_call_options` — without this, every
///   `extra_body` / typed-sampling / thinking config the user
///   wrote in `models.json` would be silently dropped.
pub fn build_api_client(
    runtime: &RuntimeConfig,
    spec: &ModelSpec,
    retry: RetryConfig,
) -> Result<Arc<ApiClient>, InferenceError> {
    let provider_cfg = runtime.providers.get(&spec.provider).ok_or_else(|| {
        crate::errors::UnknownProviderSnafu {
            provider: spec.provider.clone(),
        }
        .build()
    })?;
    let api_model_name = resolve_api_model_name(provider_cfg, &spec.model_id);
    let model_info = runtime
        .model_registry
        .resolve(&spec.provider, &spec.model_id)
        .map(|r| r.info.clone());
    let model = build_language_model_from_runtime(runtime, spec)?;
    let fingerprint = ProviderClientFingerprint::compute_with_runtime_state(
        provider_cfg,
        &api_model_name,
        &runtime.account,
        &runtime.prompt_cache,
    );
    // Per-slot cache-break detector: each `ApiClient` (primary +
    // fallbacks) gets its own detector instance. When the runtime
    // switches between slots (`ModelRuntime::on_switch_i13`), the
    // tracked state stays with the originating slot, so re-activating
    // an old slot resumes its baseline rather than restarting Cold.
    let detector = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::cache_detection::CacheBreakDetector::new(),
    ));
    let client =
        ApiClient::new(model, fingerprint, model_info, retry).with_cache_break_detector(detector);
    Ok(Arc::new(client))
}

/// Resolve the fallback chain for a role and build one `ApiClient`
/// per tier. Returns an empty vec when the role has no configured
/// fallbacks.
///
/// Fail-fast on any tier that can't construct: silently dropping a
/// fallback would only surface under outage, which is exactly when
/// the user can least afford to discover it.
pub fn build_fallback_clients_for_role(
    runtime: &RuntimeConfig,
    role: ModelRole,
    retry: RetryConfig,
) -> Result<Vec<Arc<ApiClient>>, InferenceError> {
    runtime
        .model_roles
        .fallbacks(role)
        .iter()
        .map(|spec| build_api_client(runtime, spec, retry.clone()))
        .collect()
}

/// Resolve the wire-side model name from the per-(provider, model)
/// override entry. Falls back to the `ModelSpec.model_id` when no
/// `api_model_name` is configured.
fn resolve_api_model_name(provider_cfg: &ProviderConfig, model_id: &str) -> String {
    provider_cfg
        .models
        .get(model_id)
        .and_then(|m| m.api_model_name.clone())
        .unwrap_or_else(|| model_id.to_string())
}

fn build_anthropic(
    runtime: &RuntimeConfig,
    provider_cfg: &ProviderConfig,
    api_model: &str,
    timeout_secs: i64,
    model_info: Option<&ModelInfo>,
) -> Result<Arc<dyn LanguageModel>, InferenceError> {
    let opts = &provider_cfg.client_options;
    let capabilities = anthropic_caps_from(model_info.and_then(|i| i.capabilities.as_ref()));
    let account_kind = match runtime.account.kind {
        coco_types::AccountKind::ApiKey => vercel_ai_anthropic::AdapterAccountKind::ApiKey,
        coco_types::AccountKind::ClaudeAiSubscriber => {
            vercel_ai_anthropic::AdapterAccountKind::ClaudeAiSubscriber
        }
    };
    // Parse the opaque `provider_options` map through the
    // adapter-owned schema. `deny_unknown_fields` surfaces typos at
    // process startup rather than the next request.
    let knobs = vercel_ai_anthropic::parse_provider_options(&provider_cfg.provider_options)
        .map_err(|e| {
            crate::errors::ProviderBuildFailedSnafu {
                provider: "anthropic",
                provider_name: provider_cfg.name.clone(),
                message: format!("provider_options: {e}"),
            }
            .build()
        })?;
    let settings = vercel_ai_anthropic::AnthropicProviderSettings {
        base_url: Some(provider_cfg.base_url.clone()),
        api_key: provider_cfg.resolve_api_key(),
        auth_token: opts.auth_token.as_ref().map(|t| t.expose().to_string()),
        headers: header_map(opts),
        name: Some(provider_cfg.name.clone()),
        client: build_http_client(timeout_secs),
        supports_native_structured_output: None,
        supports_strict_tools: None,
        full_url: Some(opts.full_url),
        capabilities,
        // Single-variant by design (Bedrock deferred — see
        // `vercel-ai-anthropic` `anthropic_config.rs` `ProviderTopology`).
        provider_topology: vercel_ai_anthropic::ProviderTopology::FirstParty,
        experimental_betas_enabled: knobs.experimental_betas_enabled,
        disable_interleaved_thinking: knobs.disable_interleaved_thinking,
        show_thinking_summaries: knobs.show_thinking_summaries,
        non_interactive: knobs.non_interactive,
        prompt_cache_allowlist: runtime.prompt_cache.allowlist.clone(),
        account_kind,
        in_overage: runtime.account.in_overage,
    };
    let provider = vercel_ai_anthropic::create_anthropic(settings);
    provider.language_model(api_model).map_err(|e| {
        crate::errors::ProviderBuildFailedSnafu {
            provider: "anthropic",
            provider_name: provider_cfg.name.clone(),
            message: e.to_string(),
        }
        .build()
    })
}

/// Translate `coco_types::Capability` flags into the adapter-local
/// bool-per-feature struct. Unknown capabilities (e.g. `Vision`,
/// `ToolCalling`) are ignored — only prompt-cache + beta-relevant
/// flags matter at the adapter boundary. `None` (model unknown to the
/// registry) → all-false safe default.
fn anthropic_caps_from(
    capabilities: Option<&Vec<Capability>>,
) -> vercel_ai_anthropic::AnthropicModelCapabilities {
    let mut out = vercel_ai_anthropic::AnthropicModelCapabilities::default();
    let Some(caps) = capabilities else {
        return out;
    };
    for cap in caps {
        match cap {
            Capability::PromptCache => out.prompt_cache = true,
            Capability::Context1m => out.context_1m = true,
            Capability::InterleavedThinking => out.interleaved_thinking = true,
            Capability::ContextManagement => out.context_management = true,
            Capability::TokenEfficientTools => out.token_efficient_tools = true,
            // Other capabilities are unrelated to Anthropic adapter policy.
            Capability::TextGeneration
            | Capability::Streaming
            | Capability::Vision
            | Capability::Audio
            | Capability::ToolCalling
            | Capability::Embedding
            | Capability::ExtendedThinking
            | Capability::StructuredOutput
            | Capability::ReasoningSummaries
            | Capability::ParallelToolCalls
            | Capability::FastMode => {}
        }
    }
    out
}

fn build_openai(
    provider_cfg: &ProviderConfig,
    api_model: &str,
    timeout_secs: i64,
) -> Result<Arc<dyn LanguageModel>, InferenceError> {
    let opts = &provider_cfg.client_options;
    let settings = vercel_ai_openai::OpenAIProviderSettings {
        base_url: Some(provider_cfg.base_url.clone()),
        api_key: provider_cfg.resolve_api_key(),
        organization: opts.organization_id.clone(),
        project: opts.project_id.clone(),
        headers: header_map(opts),
        name: Some(provider_cfg.name.clone()),
        client: build_http_client(timeout_secs),
        full_url: Some(opts.full_url),
    };
    let provider = vercel_ai_openai::create_openai(settings);
    // Honor `provider_cfg.wire_api`. The SDK's
    // `Provider::language_model` defaults to Responses, but users
    // who configure `wire_api: chat` (e.g. for Azure Chat Completions
    // routing) expect Chat. Dispatch explicitly.
    let model: Arc<dyn LanguageModel> = match provider_cfg.wire_api {
        WireApi::Chat => Arc::new(provider.chat(api_model)),
        WireApi::Responses => Arc::new(provider.responses(api_model)),
    };
    Ok(model)
}

fn build_google(
    provider_cfg: &ProviderConfig,
    api_model: &str,
) -> Result<Arc<dyn LanguageModel>, InferenceError> {
    let opts = &provider_cfg.client_options;
    let settings = vercel_ai_google::GoogleGenerativeAIProviderSettings {
        base_url: Some(provider_cfg.base_url.clone()),
        api_key: provider_cfg.resolve_api_key(),
        headers: header_map(opts),
        name: Some(provider_cfg.name.clone()),
    };
    let provider = vercel_ai_google::create_google_generative_ai(settings);
    provider.language_model(api_model).map_err(|e| {
        crate::errors::ProviderBuildFailedSnafu {
            provider: "google",
            provider_name: provider_cfg.name.clone(),
            message: e.to_string(),
        }
        .build()
    })
}

fn build_openai_compat(
    provider_cfg: &ProviderConfig,
    api_model: &str,
    timeout_secs: i64,
) -> Result<Arc<dyn LanguageModel>, InferenceError> {
    let opts = &provider_cfg.client_options;
    let settings = vercel_ai_openai_compatible::OpenAICompatibleProviderSettings {
        base_url: Some(provider_cfg.base_url.clone()),
        api_key: provider_cfg.resolve_api_key(),
        api_key_env_var: Some(provider_cfg.env_key.clone()),
        api_key_description: Some(provider_cfg.name.clone()),
        headers: header_map(opts),
        query_params: None,
        // The runtime instance name (e.g. `"xai"`, `"volcengine"`,
        // `"internal-router"`) is the namespace key the OpenAI-compat
        // SDK passes through; `model.provider()` answers this back so
        // it round-trips with `build_call_options`'s namespace wrap.
        name: Some(provider_cfg.name.clone()),
        client: build_http_client(timeout_secs),
        include_usage: opts.include_usage,
        supports_structured_outputs: Some(opts.supports_structured_outputs),
        transform_request_body: None,
        metadata_extractor: None,
        error_handler: None,
        full_url: Some(opts.full_url),
    };
    let provider = vercel_ai_openai_compatible::create_openai_compatible(settings);
    provider.language_model(api_model).map_err(|e| {
        crate::errors::ProviderBuildFailedSnafu {
            provider: "openai-compat",
            provider_name: provider_cfg.name.clone(),
            message: e.to_string(),
        }
        .build()
    })
}

/// Convert `ProviderClientOptions.headers` (BTreeMap, deterministic)
/// into the `HashMap` shape every `vercel-ai-*ProviderSettings.headers`
/// expects. Empty maps surface as `None` so the SDK's "no extra
/// headers" path applies.
fn header_map(opts: &ProviderClientOptions) -> Option<HashMap<String, String>> {
    if opts.headers.is_empty() {
        None
    } else {
        Some(
            opts.headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }
}

/// Effective per-request timeout for a (provider, model) pair.
/// Per-model `info.timeout_secs` overrides the provider-level value
/// when set — lets a slow-thinking model declare a higher ceiling
/// than the provider default without inflating the timeout for
/// every other model on the same provider. A negative value cannot
/// reach here (rejected at `ProviderConfig::from_partial`); zero
/// disables the per-request timeout.
fn effective_timeout_secs(provider_cfg: &ProviderConfig, model_info: Option<&ModelInfo>) -> i64 {
    model_info
        .and_then(|i| i.timeout_secs)
        .unwrap_or(provider_cfg.timeout_secs)
}

/// Build a `reqwest::Client` honoring the effective timeout and share
/// it across the language-model construction. The SDK accepts
/// `Option<Arc<reqwest::Client>>`; a single shared client lets all
/// requests against this provider reuse the connection pool. If
/// builder construction fails, falls back to the SDK's default
/// (no-timeout) client so the process still starts.
fn build_http_client(timeout_secs: i64) -> Option<Arc<reqwest::Client>> {
    let timeout = if timeout_secs > 0 {
        match u64::try_from(timeout_secs) {
            Ok(s) => Duration::from_secs(s),
            Err(_) => return None,
        }
    } else {
        // Non-positive timeout disables the per-request timeout.
        return None;
    };
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .ok()
        .map(Arc::new)
}

#[cfg(test)]
#[path = "model_factory.test.rs"]
mod tests;
