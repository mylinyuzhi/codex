//! Gemini adapter for OpenAPI-style Gemini models
//!
//! This is a placeholder implementation for future Gemini support.
//! All methods return unsupported errors until full implementation.
//!
//! # Planned Features
//!
//! - Support for Gemini models via OpenAPI-compatible endpoints
//! - Incremental message sending with previous_response_id
//! - Streaming and non-streaming modes
//!

use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestContext;
use crate::adapters::RequestMetadata;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::CodexErr;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use serde_json::Value as JsonValue;

/// Gemini adapter (placeholder - not yet implemented)
///
/// This adapter is reserved for future Gemini model support via
/// OpenAPI-compatible endpoints. All methods currently return
/// "not yet implemented" errors.
#[derive(Debug, Clone)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    /// Create a new Gemini adapter instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini_openapi"
    }

    fn validate_provider(&self, _provider: &ModelProviderInfo) -> Result<()> {
        Err(CodexErr::Fatal(
            "GeminiAdapter is not yet implemented. \
             This is a placeholder for future Gemini support via OpenAPI-compatible endpoints. \
             Please use a different adapter or provider."
                .to_string(),
        ))
    }

    fn transform_request(
        &self,
        _prompt: &Prompt,
        _context: &RequestContext,
        _provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        Err(CodexErr::Fatal(
            "GeminiAdapter.transform_request() not yet implemented".to_string(),
        ))
    }

    fn transform_response_chunk(
        &self,
        _chunk: &str,
        _context: &mut AdapterContext,
        _provider: &ModelProviderInfo,
    ) -> Result<Vec<ResponseEvent>> {
        Err(CodexErr::Fatal(
            "GeminiAdapter.transform_response_chunk() not yet implemented".to_string(),
        ))
    }

    fn supports_previous_response_id(&self) -> bool {
        // Will be true once implemented
        false
    }

    fn endpoint_path(&self) -> Option<&str> {
        // Will return Gemini-specific path once implemented
        None
    }

    fn build_request_metadata(
        &self,
        _prompt: &Prompt,
        _provider: &ModelProviderInfo,
        _context: &RequestContext,
    ) -> Result<RequestMetadata> {
        Err(CodexErr::Fatal(
            "GeminiAdapter.build_request_metadata() not yet implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_adapter_name() {
        let adapter = GeminiAdapter::new();
        assert_eq!(adapter.name(), "gemini_openapi");
    }

    #[test]
    fn test_gemini_adapter_not_yet_supported() {
        let adapter = GeminiAdapter::new();
        assert!(!adapter.supports_previous_response_id());
    }

    #[test]
    fn test_gemini_adapter_returns_unsupported_error() {
        let adapter = GeminiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.name = "test".to_string();
        provider.ext.adapter = Some("gemini".to_string());

        let result = adapter.validate_provider(&provider);
        assert!(result.is_err());

        if let Err(err) = result {
            let err_msg = err.to_string();
            assert!(err_msg.contains("not yet implemented"));
        }
    }

    #[test]
    fn test_gemini_adapter_default() {
        let adapter = GeminiAdapter::default();
        assert_eq!(adapter.name(), "gemini_openapi");
    }
}
