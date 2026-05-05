//! Shared scaffolding for live integration tests.
//!
//! Tests in this crate exercise the **full coco-rs chain**:
//!
//! ```text
//!  builtin_providers()  →  RuntimeConfig  →  ModelRegistry
//!     →  model_factory::build_language_model_from_runtime
//!     →  vercel-ai SDK / coco_inference::ApiClient
//!     →  real provider HTTP API
//! ```
//!
//! The framework auto-skips when credentials are missing — tests print a
//! one-line reason and return `Ok(())` so unconfigured CI stays green.

// Each `tests/<runner>.rs` includes this module via `mod common;` and
// only consumes the subset it needs. `dead_code` and `unused_imports`
// are silenced so a runner that doesn't touch `fixtures` / `build_client`
// doesn't produce per-binary warnings.
#![allow(dead_code, unused_imports)]

pub mod env;
pub mod fixtures;
pub mod runtime;
pub mod tmpdir;
pub mod usage_report;

pub use fixtures::*;
pub use runtime::LiveTarget;
pub use runtime::build_client;
pub use runtime::provider_has_credentials;

/// Resolve `(provider, model)` into a `LiveTarget` or return `Ok(())`
/// from the calling test with a one-line skip message.
///
/// Skip cases:
/// - `COCO_LIVE_TEST_PROVIDERS` is set and excludes this provider
/// - The provider's `env_key` (e.g. `DEEPSEEK_API_KEY`) is not present
///   in the process env
/// - The named capability is disabled by `COCO_LIVE_TEST_CAPABILITIES`
///
/// Usage:
/// ```ignore
/// #[tokio::test]
/// async fn test_basic_deepseek_openai() -> anyhow::Result<()> {
///     let target = require_live!("deepseek-openai", "deepseek-v4-flash", "text");
///     suite::basic::run(&target).await
/// }
/// ```
#[macro_export]
macro_rules! require_live {
    ($provider:expr, $model:expr, $capability:expr) => {{
        if !$crate::common::env::capability_enabled($capability) {
            eprintln!(
                "[skip] capability `{}` disabled by COCO_LIVE_TEST_CAPABILITIES",
                $capability
            );
            return Ok(());
        }
        match $crate::common::LiveTarget::try_resolve($provider, $model) {
            None => {
                eprintln!(
                    "[skip] provider `{}` not available (missing API key or excluded by \
                     COCO_LIVE_TEST_PROVIDERS)",
                    $provider
                );
                return Ok(());
            }
            Some(Err(e)) => {
                return Err(anyhow::anyhow!(
                    "failed to build live target {}/{}: {e}",
                    $provider,
                    $model
                ));
            }
            Some(Ok(target)) => target,
        }
    }};
}
