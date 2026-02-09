//! HTTP client for the OpenAI API.

use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;
use futures::stream::Stream;
use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use serde::de::DeserializeOwned;

use crate::config::ClientConfig;
use crate::config::HttpRequest;
use crate::error::OpenAIError;
use crate::error::Result;
use crate::resources::Embeddings;
use crate::resources::Responses;
use crate::types::Response;
use crate::types::SdkHttpResponse;

/// Environment variable for API key.
const API_KEY_ENV: &str = "OPENAI_API_KEY";

/// The OpenAI API client.
#[derive(Debug, Clone)]
pub struct Client {
    http_client: reqwest::Client,
    config: ClientConfig,
}

impl Client {
    /// Create a new client with the given configuration.
    pub fn new(config: ClientConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(OpenAIError::Configuration(
                "API key is required".to_string(),
            ));
        }

        let http_client = reqwest::Client::builder().timeout(config.timeout).build()?;

        Ok(Self {
            http_client,
            config,
        })
    }

    /// Create a new client using the OPENAI_API_KEY environment variable.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(API_KEY_ENV).map_err(|_| {
            OpenAIError::Configuration(format!("Missing {API_KEY_ENV} environment variable"))
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

        // Add optional organization header
        if let Some(org) = &self.config.organization {
            if let Ok(value) = HeaderValue::from_str(org) {
                headers.insert("OpenAI-Organization", value);
            }
        }

        // Add optional project header
        if let Some(project) = &self.config.project {
            if let Ok(value) = HeaderValue::from_str(project) {
                headers.insert("OpenAI-Project", value);
            }
        }

        headers
    }

    /// Apply request hook if configured.
    fn apply_hook(
        &self,
        url: String,
        headers: HeaderMap,
        body: serde_json::Value,
    ) -> (String, HeaderMap, serde_json::Value) {
        if let Some(hook) = &self.config.request_hook {
            // Convert HeaderMap to HashMap for hook
            let header_map: HashMap<String, String> = headers
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|val| (k.to_string(), val.to_string())))
                .collect();

            let mut http_request = HttpRequest {
                url,
                headers: header_map,
                body,
            };

            // Call the hook
            hook.on_request(&mut http_request);

            // Convert HashMap back to HeaderMap
            let mut new_headers = HeaderMap::new();
            for (k, v) in http_request.headers {
                if let (Ok(name), Ok(value)) = (
                    reqwest::header::HeaderName::try_from(k.as_str()),
                    HeaderValue::from_str(&v),
                ) {
                    new_headers.insert(name, value);
                }
            }

            (http_request.url, new_headers, http_request.body)
        } else {
            (url, headers, body)
        }
    }

    /// Send a POST request to the API.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let base_url = format!("{}{}", self.config.base_url, path);
        let (url, headers, body) = self.apply_hook(base_url, self.default_headers(), body);
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
                .headers(headers.clone())
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
                        return resp.json::<T>().await.map_err(OpenAIError::from);
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
                    let error = OpenAIError::Network(e);
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

    /// Send a GET request that returns Response with sdk_http_response populated.
    pub(crate) async fn get_response(&self, path: &str) -> Result<Response> {
        let base_url = format!("{}{}", self.config.base_url, path);
        let (url, headers, _) =
            self.apply_hook(base_url, self.default_headers(), serde_json::json!({}));
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2_u64.pow(attempt as u32 - 1));
                tokio::time::sleep(delay).await;
            }

            let response = self
                .http_client
                .get(&url)
                .headers(headers.clone())
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
                        // Capture raw body before deserializing
                        let body_text = resp.text().await.map_err(OpenAIError::from)?;
                        let mut result: Response =
                            serde_json::from_str(&body_text).map_err(|e| {
                                OpenAIError::Parse(format!(
                                    "Failed to parse response: {e}\nBody: {body_text}"
                                ))
                            })?;

                        // Store raw response body for round-trip preservation
                        result.sdk_http_response = Some(SdkHttpResponse::from_status_and_body(
                            status.as_u16() as i32,
                            body_text,
                        ));

                        return Ok(result);
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
                    let error = OpenAIError::Network(e);
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

    /// Send a POST request that returns Response with sdk_http_response populated.
    ///
    /// This is a specialized version of `post()` that captures the raw HTTP response
    /// body and stores it in `Response.sdk_http_response` for round-trip preservation.
    pub(crate) async fn post_response(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<Response> {
        let base_url = format!("{}{}", self.config.base_url, path);
        let (url, headers, body) = self.apply_hook(base_url, self.default_headers(), body);
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
                .headers(headers.clone())
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
                        // Capture raw body before deserializing
                        let body_text = resp.text().await.map_err(OpenAIError::from)?;
                        let mut result: Response =
                            serde_json::from_str(&body_text).map_err(|e| {
                                OpenAIError::Parse(format!(
                                    "Failed to parse response: {e}\nBody: {body_text}"
                                ))
                            })?;

                        // Store raw response body for round-trip preservation
                        result.sdk_http_response = Some(SdkHttpResponse::from_status_and_body(
                            status.as_u16() as i32,
                            body_text,
                        ));

                        return Ok(result);
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
                    let error = OpenAIError::Network(e);
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

    /// Send a POST request that returns a byte stream for SSE processing.
    ///
    /// This method is used for streaming responses. Unlike `post()`, it does not
    /// deserialize the response but instead returns the raw byte stream that can
    /// be processed by the SSE decoder.
    ///
    /// Note: This method does not support retry logic since streaming responses
    /// cannot be easily retried.
    pub(crate) async fn post_stream(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<impl Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static>
    {
        let base_url = format!("{}{}", self.config.base_url, path);
        let (url, headers, body) = self.apply_hook(base_url, self.default_headers(), body);

        let response = self
            .http_client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &error_body, request_id));
        }

        Ok(response.bytes_stream())
    }

    /// Send a GET request that returns a byte stream for SSE processing.
    ///
    /// This method is used for streaming responses from existing response IDs,
    /// such as when resuming an interrupted stream.
    ///
    /// Note: This method does not support retry logic since streaming responses
    /// cannot be easily retried.
    pub(crate) async fn get_stream(
        &self,
        path: &str,
    ) -> Result<impl Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static>
    {
        let base_url = format!("{}{}", self.config.base_url, path);
        let (url, headers, _) =
            self.apply_hook(base_url, self.default_headers(), serde_json::json!({}));

        let response = self.http_client.get(&url).headers(headers).send().await?;

        let status = response.status();
        if !status.is_success() {
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &error_body, request_id));
        }

        Ok(response.bytes_stream())
    }
}

/// Parse an API error response.
fn parse_api_error(status: u16, body: &str, request_id: Option<String>) -> OpenAIError {
    // Try to parse structured error
    if let Ok(error_response) = serde_json::from_str::<ApiErrorResponse>(body) {
        let message = error_response.error.message;
        let code = error_response.error.code.as_deref().unwrap_or("");

        // Map specific error codes
        if code.contains("context_length_exceeded") {
            return OpenAIError::ContextWindowExceeded;
        }
        if code.contains("insufficient_quota") {
            return OpenAIError::QuotaExceeded;
        }
        if code.contains("previous_response_not_found") {
            return OpenAIError::PreviousResponseNotFound;
        }

        match status {
            400 => OpenAIError::BadRequest(message),
            401 => OpenAIError::Authentication(message),
            429 => OpenAIError::RateLimited { retry_after: None },
            500..=599 => OpenAIError::InternalServerError,
            _ => OpenAIError::Api {
                status,
                message,
                request_id,
            },
        }
    } else {
        OpenAIError::Api {
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
#[path = "client.test.rs"]
mod tests;
