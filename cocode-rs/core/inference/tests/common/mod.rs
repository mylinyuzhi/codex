//! Test helpers and macros for cocode-inference live integration tests.

pub mod config;
pub mod fixtures;

pub use config::load_provider_config;

/// Macro to require a provider, returning (ApiClient, Arc<dyn LanguageModel>, ProviderTestConfig)
/// or skipping the test if the provider is not configured.
#[macro_export]
macro_rules! require_api_provider {
    ($provider:expr) => {
        match $crate::common::load_provider_config($provider) {
            Some(cfg) if cfg.enabled => {
                match cocode_inference::ApiClient::from_provider_info(
                    &cfg.provider_info,
                    &cfg.model_slug,
                    cocode_inference::ApiClientConfig::default(),
                ) {
                    Ok((client, model)) => (client, model, cfg),
                    Err(e) => {
                        eprintln!(
                            "Skipping test: failed to create provider '{}': {e}",
                            $provider
                        );
                        return Ok(());
                    }
                }
            }
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        }
    };
    ($provider:expr, $capability:expr) => {
        match $crate::common::load_provider_config($provider) {
            Some(cfg) if cfg.enabled => {
                if !cfg.has_capability($capability) {
                    eprintln!(
                        "Skipping test: capability '{}' not enabled for provider '{}'",
                        $capability, $provider
                    );
                    return Ok(());
                }
                match cocode_inference::ApiClient::from_provider_info(
                    &cfg.provider_info,
                    &cfg.model_slug,
                    cocode_inference::ApiClientConfig::default(),
                ) {
                    Ok((client, model)) => (client, model, cfg),
                    Err(e) => {
                        eprintln!(
                            "Skipping test: failed to create provider '{}': {e}",
                            $provider
                        );
                        return Ok(());
                    }
                }
            }
            _ => {
                eprintln!(
                    "Skipping test: provider '{}' not configured in .env",
                    $provider
                );
                return Ok(());
            }
        }
    };
}
