//! HTTP client for the Volcengine Ark API.

use std::time::Duration;

use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use serde::de::DeserializeOwned;

use crate::config::ClientConfig;
use crate::error::ArkError;
use crate::error::Result;
use crate::resources::Embeddings;
use crate::resources::Responses;

/// Environment variable for API key.
const API_KEY_ENV: &str = "ARK_API_KEY";

/// The Volcengine Ark API client.
#[derive(Debug, Clone)]
pub struct Client {
    http_client: reqwest::Client,
    config: ClientConfig,
}

impl Client {
    /// Create a new client with the given configuration.
    pub fn new(config: ClientConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(ArkError::Configuration("API key is required".to_string()));
        }

        let http_client = reqwest::Client::builder().timeout(config.timeout).build()?;

        Ok(Self {
            http_client,
            config,
        })
    }

    /// Create a new client using the ARK_API_KEY environment variable.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(API_KEY_ENV).map_err(|_| {
            ArkError::Configuration(format!("Missing {API_KEY_ENV} environment variable"))
        })?;

        Self::new(ClientConfig::new(api_key))
    }

    /// Create a new client with the given API key.
    pub fn with_api_key(api_key: impl Into<String>) -> Result<Self> {
        Self::new(ClientConfig::new(api_key))
    }

    /// Get the responses resource.
    pub fn responses(&self) -> Responses<'_> {
        Responses::new(self)
    }

    /// Get the embeddings resource.
    pub fn embeddings(&self) -> Embeddings<'_> {
        Embeddings::new(self)
    }

    /// Build the default headers for API requests.
    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
                .expect("valid api key"),
        );
        headers
    }

    /// Send a POST request to the API.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let url = format!("{}{}", self.config.base_url, path);
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2_u64.pow(attempt as u32 - 1));
                tokio::time::sleep(delay).await;
            }

            let response = self
                .http_client
                .post(&url)
                .headers(self.default_headers())
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let request_id = resp
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(String::from);

                    if status.is_success() {
                        return resp.json::<T>().await.map_err(ArkError::from);
                    }

                    // Try to parse error response
                    let error_body = resp.text().await.unwrap_or_default();
                    let error = parse_api_error(status.as_u16(), &error_body, request_id);

                    // Check if retryable
                    if error.is_retryable() && attempt < self.config.max_retries {
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
                Err(e) => {
                    let error = ArkError::Network(e);
                    if error.is_retryable() && attempt < self.config.max_retries {
                        last_error = Some(error);
                        continue;
                    }
                    return Err(error);
                }
            }
        }

        Err(last_error.expect("at least one error should have occurred"))
    }
}

/// Parse an API error response.
fn parse_api_error(status: u16, body: &str, request_id: Option<String>) -> ArkError {
    // Try to parse structured error
    if let Ok(error_response) = serde_json::from_str::<ApiErrorResponse>(body) {
        let message = error_response.error.message;
        let code = error_response.error.code.as_deref().unwrap_or("");

        // Map specific error codes
        if code.contains("context_length_exceeded") {
            return ArkError::ContextWindowExceeded;
        }
        if code.contains("insufficient_quota") {
            return ArkError::QuotaExceeded;
        }
        if code.contains("previous_response_not_found") {
            return ArkError::PreviousResponseNotFound;
        }

        match status {
            400 => ArkError::BadRequest(message),
            401 => ArkError::Authentication(message),
            429 => ArkError::RateLimited { retry_after: None },
            500..=599 => ArkError::InternalServerError,
            _ => ArkError::Api {
                status,
                message,
                request_id,
            },
        }
    } else {
        ArkError::Api {
            status,
            message: body.to_string(),
            request_id,
        }
    }
}

/// API error response structure.
#[derive(Debug, serde::Deserialize)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

#[derive(Debug, serde::Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    code: Option<String>,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_requires_api_key() {
        let result = Client::new(ClientConfig::default());
        assert!(matches!(result, Err(ArkError::Configuration(_))));
    }

    #[test]
    fn test_client_with_api_key() {
        let result = Client::with_api_key("test-key");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_api_error_structured() {
        let body = r#"{"error":{"code":"invalid_request_error","message":"Invalid model"}}"#;
        let error = parse_api_error(400, body, None);
        assert!(matches!(error, ArkError::BadRequest(_)));
    }

    #[test]
    fn test_parse_api_error_rate_limit() {
        let body = r#"{"error":{"code":"rate_limit_error","message":"Rate limited"}}"#;
        let error = parse_api_error(429, body, None);
        assert!(matches!(error, ArkError::RateLimited { .. }));
    }

    #[test]
    fn test_parse_api_error_context_exceeded() {
        let body = r#"{"error":{"code":"context_length_exceeded","message":"Context too long"}}"#;
        let error = parse_api_error(400, body, None);
        assert!(matches!(error, ArkError::ContextWindowExceeded));
    }
}
