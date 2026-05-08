//! Build a `RuntimeConfig` populated only with builtin providers and
//! resolve a `(provider, model)` pair into an `Arc<ApiClient>` — the
//! single seam-approved entry point into the AI SDK chain. Tests call
//! `client.query` / `client.query_stream` directly; provider-direct
//! `vercel-ai` SDK access is forbidden by the seam guard.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Context;
use anyhow::Result;
use coco_config::CatalogPaths;
use coco_config::ProviderConfig;
use coco_config::RuntimeConfig;
use coco_config::RuntimeConfigBuilder;
use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_inference::model_factory::build_api_client;
use coco_types::ModelSpec;

use crate::common::env::ensure_env_loaded;

/// Cached runtime — building it touches the filesystem (tempdir creation
/// and provider resolution), so we share one across all tests in a
/// process. Tests do not mutate it; `build_*` helpers only read.
static RUNTIME: OnceLock<TestRuntime> = OnceLock::new();

/// Owns the tempdir whose lifetime backs `RuntimeConfig.paths` so it
/// outlives every test in the process.
struct TestRuntime {
    runtime: Arc<RuntimeConfig>,
    _home: tempfile::TempDir,
}

/// Build (or fetch the cached) `RuntimeConfig` whose only providers are
/// the compile-time builtins. No `~/.coco/providers.json` overlay, no
/// `settings.json`, no managed policy file — the catalog paths point at
/// a fresh tempdir whose files don't exist.
///
/// Live `DEEPSEEK_API_KEY` (etc.) flows through because we use
/// `EnvSnapshot::from_current_process()`. The `.env` file loaded by
/// `ensure_env_loaded()` populates `std::env` first, so subsequent
/// `EnvSnapshot::from_current_process()` calls see those values.
pub fn shared_runtime() -> &'static Arc<RuntimeConfig> {
    let entry = RUNTIME.get_or_init(|| {
        ensure_env_loaded();
        let home = tempfile::tempdir().expect("create test tempdir");
        let runtime = RuntimeConfigBuilder::from_process(home.path())
            .with_catalog_paths(CatalogPaths::empty_in(home.path()))
            .build()
            .expect("build empty-overlay runtime");
        TestRuntime {
            runtime: Arc::new(runtime),
            _home: home,
        }
    });
    &entry.runtime
}

/// Borrow a builtin provider config from the shared runtime. Borrow is
/// tied to the static `RuntimeConfig` (lives for the whole process) so
/// no clone is needed.
pub fn provider_config(name: &str) -> Option<&'static ProviderConfig> {
    shared_runtime().providers.get(name)
}

/// Construct a `ModelSpec` whose `api` matches the provider's builtin
/// declaration. This is the same shape the production codepath produces
/// when resolving a `models.<role>` entry.
pub fn spec_for(provider_name: &str, model_id: &str) -> Result<ModelSpec> {
    let cfg = provider_config(provider_name)
        .with_context(|| format!("provider `{provider_name}` is not a builtin"))?;
    Ok(ModelSpec {
        provider: cfg.name.clone(),
        api: cfg.api,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
    })
}

/// `true` when the provider's `env_key` is set to a non-empty value in
/// the live process environment. Mirrors `ProviderConfig::resolve_api_key`
/// without surfacing the secret.
pub fn provider_has_credentials(provider_name: &str) -> bool {
    ensure_env_loaded();
    provider_config(provider_name)
        .and_then(coco_config::ProviderConfig::resolve_api_key)
        .is_some()
}

/// Build an `ApiClient` for `(provider, model)` so suites can exercise
/// `coco-inference` retry, usage accumulation, and cache-break wiring.
/// All AI SDK access flows through this — direct `vercel-ai` use is
/// blocked by the workspace seam guard.
pub fn build_client(provider_name: &str, model_id: &str) -> Result<Arc<ApiClient>> {
    let runtime = shared_runtime();
    let spec = spec_for(provider_name, model_id)?;
    Ok(build_api_client(runtime, &spec, RetryConfig::default())?)
}

/// Resolved test target — provider name, the model-id under test, and a
/// pre-built `Arc<ApiClient>`.
pub struct LiveTarget {
    pub provider: &'static str,
    pub model: String,
    pub client: Arc<ApiClient>,
}

impl LiveTarget {
    /// Build a target for `(provider, model)`. `None` when credentials
    /// are missing or the provider is not on the allow-list.
    pub fn try_resolve(provider: &'static str, model: &str) -> Option<Result<Self>> {
        if !crate::common::env::provider_allowed(provider) {
            return None;
        }
        if !provider_has_credentials(provider) {
            return None;
        }
        Some(
            build_client(provider, model)
                .map(|client| LiveTarget {
                    provider,
                    model: model.to_string(),
                    client,
                })
                .with_context(|| format!("build_client({provider}, {model})")),
        )
    }
}

/// Optional override path printed by skip messages so users know what
/// they need to set. Returned for diagnostic prints only.
pub fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
