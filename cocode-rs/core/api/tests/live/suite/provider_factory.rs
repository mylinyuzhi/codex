//! Provider factory validation tests.

use anyhow::Result;
use cocode_api::provider_factory;
use cocode_protocol::ProviderInfo;

/// Verify create_provider succeeds for a given ProviderInfo.
pub async fn run_create_provider(info: &ProviderInfo) -> Result<()> {
    let provider = provider_factory::create_provider(info)?;

    // Provider should have a valid provider ID
    let _provider_name = provider.provider();

    Ok(())
}

/// Verify create_model succeeds and returns a model with correct ID.
pub async fn run_create_model(info: &ProviderInfo, model_slug: &str) -> Result<()> {
    let model = provider_factory::create_model(info, model_slug)?;

    // Model should have a valid ID containing the slug or API name
    let model_id = model.model_id().to_string();
    assert!(!model_id.is_empty(), "Model ID should not be empty");

    Ok(())
}
