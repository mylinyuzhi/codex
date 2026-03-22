//! API posting and getting utilities.

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use reqwest::Response;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::APICallError;

use crate::response_handler::ResponseHandler;

/// Type alias for boxed byte streams.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

/// Convert an APICallError to AISdkError.
fn api_error_to_sdk_error(error: APICallError) -> AISdkError {
    let message = error.message.clone();
    AISdkError::new(message).with_cause(Box::new(error))
}

/// Check if the cancellation token is triggered.
fn check_cancelled(signal: &Option<CancellationToken>) -> Result<(), AISdkError> {
    if let Some(token) = signal
        && token.is_cancelled()
    {
        return Err(AISdkError::new("Request was cancelled"));
    }
    Ok(())
}

/// Get or create a default HTTP client.
fn get_client(client: &Option<Arc<reqwest::Client>>) -> reqwest::Client {
    client.as_ref().map(|c| (**c).clone()).unwrap_or_default()
}

/// POST JSON data to an API endpoint.
pub async fn post_json_to_api<T>(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
    abort_signal: Option<CancellationToken>,
) -> Result<T, AISdkError> {
    post_json_to_api_with_client(
        url,
        headers,
        body,
        success_handler,
        error_handler,
        abort_signal,
        None,
    )
    .await
}

/// POST JSON data to an API endpoint with a custom client.
///
/// # Arguments
///
/// * `url` - The API endpoint URL.
/// * `headers` - Optional headers to include in the request.
/// * `body` - The JSON body to send.
/// * `success_handler` - Handler for successful responses.
/// * `error_handler` - Handler for error responses.
/// * `abort_signal` - Optional cancellation token.
/// * `client` - Optional custom HTTP client for connection pooling.
pub async fn post_json_to_api_with_client<T>(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
    abort_signal: Option<CancellationToken>,
    client: Option<Arc<reqwest::Client>>,
) -> Result<T, AISdkError> {
    check_cancelled(&abort_signal)?;

    let client = get_client(&client);
    let mut request = client.post(url).json(body);

    if let Some(h) = headers {
        for (key, value) in h {
            request = request.header(&key, &value);
        }
    }

    let response = request.send().await.map_err(|e| {
        api_error_to_sdk_error(APICallError::new(format!("Request failed: {e}"), url))
    })?;

    check_cancelled(&abort_signal)?;
    handle_response(response, url, body.clone(), success_handler, error_handler).await
}

/// GET data from an API endpoint.
pub async fn get_from_api<T>(
    url: &str,
    headers: Option<HashMap<String, String>>,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
    abort_signal: Option<CancellationToken>,
) -> Result<T, AISdkError> {
    get_from_api_with_client(
        url,
        headers,
        success_handler,
        error_handler,
        abort_signal,
        None,
    )
    .await
}

/// GET data from an API endpoint with a custom client.
///
/// # Arguments
///
/// * `url` - The API endpoint URL.
/// * `headers` - Optional headers to include in the request.
/// * `success_handler` - Handler for successful responses.
/// * `error_handler` - Handler for error responses.
/// * `abort_signal` - Optional cancellation token.
/// * `client` - Optional custom HTTP client for connection pooling.
pub async fn get_from_api_with_client<T>(
    url: &str,
    headers: Option<HashMap<String, String>>,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
    abort_signal: Option<CancellationToken>,
    client: Option<Arc<reqwest::Client>>,
) -> Result<T, AISdkError> {
    check_cancelled(&abort_signal)?;

    let client = get_client(&client);
    let mut request = client.get(url);

    if let Some(h) = headers {
        for (key, value) in h {
            request = request.header(&key, &value);
        }
    }

    let response = request.send().await.map_err(|e| {
        api_error_to_sdk_error(APICallError::new(format!("Request failed: {e}"), url))
    })?;

    handle_response(response, url, Value::Null, success_handler, error_handler).await
}

async fn handle_response<T>(
    response: Response,
    url: &str,
    request_body: Value,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
) -> Result<T, AISdkError> {
    let status = response.status();
    let url = url.to_string();

    if status.is_success() {
        success_handler.handle(response, &url, &request_body).await
    } else {
        let error = error_handler
            .handle(response, &url, &request_body)
            .await
            .unwrap_or_else(|e| {
                api_error_to_sdk_error(
                    APICallError::new(
                        format!("HTTP {status} error, and error handler failed: {e}"),
                        &url,
                    )
                    .with_status(status.as_u16()),
                )
            });
        Err(error)
    }
}

/// POST and return a stream.
pub async fn post_stream_to_api(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    abort_signal: Option<CancellationToken>,
) -> Result<ByteStream, AISdkError> {
    post_stream_to_api_with_client(url, headers, body, abort_signal, None).await
}

/// POST and return a stream with a custom client.
///
/// # Arguments
///
/// * `url` - The API endpoint URL.
/// * `headers` - Optional headers to include in the request.
/// * `body` - The JSON body to send.
/// * `abort_signal` - Optional cancellation token.
/// * `client` - Optional custom HTTP client for connection pooling.
pub async fn post_stream_to_api_with_client(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    abort_signal: Option<CancellationToken>,
    client: Option<Arc<reqwest::Client>>,
) -> Result<ByteStream, AISdkError> {
    check_cancelled(&abort_signal)?;

    let client = get_client(&client);
    let mut request = client.post(url).json(body);

    if let Some(h) = headers {
        for (key, value) in h {
            request = request.header(&key, &value);
        }
    }

    let response = request.send().await.map_err(|e| {
        api_error_to_sdk_error(APICallError::new(format!("Request failed: {e}"), url))
    })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(api_error_to_sdk_error(
            APICallError::new(format!("HTTP {status}: {body}"), url)
                .with_status(status.as_u16())
                .with_response_body(body),
        ));
    }

    Ok(Box::pin(response.bytes_stream()))
}

/// API response wrapper that includes HTTP response headers.
pub struct ApiResponse<T> {
    /// The parsed response value.
    pub value: T,
    /// HTTP response headers.
    pub headers: HashMap<String, String>,
}

/// Extract response headers into a HashMap.
fn extract_response_headers(response: &Response) -> HashMap<String, String> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect()
}

/// POST JSON data to an API endpoint with a custom client, returning response headers.
pub async fn post_json_to_api_with_client_and_headers<T>(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    success_handler: impl ResponseHandler<T>,
    error_handler: impl ResponseHandler<AISdkError>,
    abort_signal: Option<CancellationToken>,
    client: Option<Arc<reqwest::Client>>,
) -> Result<ApiResponse<T>, AISdkError> {
    check_cancelled(&abort_signal)?;

    let client = get_client(&client);
    let mut request = client.post(url).json(body);

    if let Some(h) = headers {
        for (key, value) in h {
            request = request.header(&key, &value);
        }
    }

    let response = request.send().await.map_err(|e| {
        api_error_to_sdk_error(APICallError::new(format!("Request failed: {e}"), url))
    })?;

    check_cancelled(&abort_signal)?;

    let response_headers = extract_response_headers(&response);
    let value =
        handle_response(response, url, body.clone(), success_handler, error_handler).await?;
    Ok(ApiResponse {
        value,
        headers: response_headers,
    })
}

/// POST and return a stream with a custom client, also returning response headers.
pub async fn post_stream_to_api_with_client_and_headers(
    url: &str,
    headers: Option<HashMap<String, String>>,
    body: &Value,
    abort_signal: Option<CancellationToken>,
    client: Option<Arc<reqwest::Client>>,
) -> Result<(ByteStream, HashMap<String, String>), AISdkError> {
    check_cancelled(&abort_signal)?;

    let client = get_client(&client);
    let mut request = client.post(url).json(body);

    if let Some(h) = headers {
        for (key, value) in h {
            request = request.header(&key, &value);
        }
    }

    let response = request.send().await.map_err(|e| {
        api_error_to_sdk_error(APICallError::new(format!("Request failed: {e}"), url))
    })?;

    let status = response.status();
    let response_headers = extract_response_headers(&response);

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(api_error_to_sdk_error(
            APICallError::new(format!("HTTP {status}: {body}"), url)
                .with_status(status.as_u16())
                .with_response_body(body),
        ));
    }

    Ok((Box::pin(response.bytes_stream()), response_headers))
}

/// Error handler trait for API errors.
#[async_trait]
pub trait ErrorHandler: Send + Sync {
    async fn handle(&self, response: Response, url: &str, request_body: &Value) -> AISdkError;
}

/// Default error handler.
pub struct DefaultErrorHandler;

#[async_trait]
impl ErrorHandler for DefaultErrorHandler {
    async fn handle(&self, response: Response, url: &str, _request_body: &Value) -> AISdkError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        let is_retryable = status.as_u16() == 429 || status.as_u16() >= 500;

        api_error_to_sdk_error(
            APICallError::new(format!("HTTP {status}: {body}"), url)
                .with_status(status.as_u16())
                .with_response_body(body)
                .with_retryable(is_retryable),
        )
    }
}

/// API error type.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Request cancelled")]
    Cancelled,
}
