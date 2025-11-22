use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use codex_client::RequestTelemetry;
use codex_client::RetryOn;
use codex_client::RetryPolicy;
use codex_client::TransportError;
use codex_client::backoff;
use eventsource_stream::Eventsource;
use futures::prelude::*;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;

use codex_otel::otel_event_manager::OtelEventManager;

use crate::adapters::AdapterConfig;
use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestMetadata;
use crate::adapters::get_adapter;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::default_client::build_reqwest_client;
use crate::error::CodexErr;
use crate::error::Result;
use crate::error::RetryLimitReachedError;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::WireApi;

/// Error response structure from provider APIs
#[derive(Debug, Deserialize, Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

/// Error detail structure
#[derive(Debug, Deserialize, Serialize)]
struct ErrorDetail {
    code: Option<String>,
    message: Option<String>,
}

/// Telemetry adapter bridging codex-client RequestTelemetry to OtelEventManager.
struct AdapterTelemetry {
    otel_event_manager: OtelEventManager,
}

impl AdapterTelemetry {
    fn new(otel_event_manager: OtelEventManager) -> Self {
        Self { otel_event_manager }
    }
}

impl RequestTelemetry for AdapterTelemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<StatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let error_message = error.map(std::string::ToString::to_string);
        self.otel_event_manager.record_api_request(
            attempt,
            status.map(|s| s.as_u16()),
            error_message.as_deref(),
            duration,
        );
    }
}

/// Build retry policy from provider configuration.
///
/// Uses existing `ModelProviderInfo` fields:
/// - `request_max_retries` - max retry attempts (default: 3)
/// - Base delay is fixed at 200ms to match codex-api behavior
fn build_retry_policy(provider: &ModelProviderInfo) -> RetryPolicy {
    RetryPolicy {
        max_attempts: provider.request_max_retries.unwrap_or(3),
        base_delay: Duration::from_millis(200), // Match codex-api default
        retry_on: RetryOn {
            retry_429: true, // Always retry rate limits
            retry_5xx: true, // Always retry server errors
            retry_transport: true,
        },
    }
}

/// Map TransportError to CodexErr.
fn map_transport_error(err: TransportError) -> CodexErr {
    match err {
        TransportError::RetryLimit => CodexErr::RetryLimit(RetryLimitReachedError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            request_id: None,
        }),
        TransportError::Timeout => CodexErr::Timeout,
        TransportError::Network(msg) | TransportError::Build(msg) => CodexErr::Stream(msg, None),
        TransportError::Http { status, body, .. } => {
            CodexErr::Fatal(format!("HTTP error {status}: {}", body.unwrap_or_default()))
        }
    }
}

/// HTTP client for adapter-based provider communication
///
/// Handles HTTP request/response transformation and streaming for providers
/// that use custom adapters (non-OpenAI providers).
pub(crate) struct AdapterHttpClient {
    otel_event_manager: OtelEventManager,
}

impl AdapterHttpClient {
    pub fn new(otel_event_manager: OtelEventManager) -> Self {
        Self { otel_event_manager }
    }

    /// Stream with a custom adapter
    ///
    /// Main entry point for adapter-based streaming. Handles:
    /// - Adapter configuration and validation
    /// - Request transformation
    /// - Routing to appropriate wire API (Chat/Responses)
    pub async fn stream_with_adapter(
        &self,
        prompt: &Prompt,
        context: crate::adapters::RequestContext,
        provider: &ModelProviderInfo,
        adapter_name: &str,
        global_stream_idle_timeout: Option<u64>,
    ) -> Result<ResponseStream> {
        // Get adapter from registry
        let mut adapter = get_adapter(adapter_name)
            .map_err(|e| CodexErr::Fatal(format!("Failed to get adapter '{adapter_name}': {e}")))?;

        // Configure adapter if provider has adapter_config
        if let Some(config_map) = &provider.ext.adapter_config {
            let mut config = AdapterConfig::new();
            config.options = config_map.clone();

            // Configure and validate
            Arc::get_mut(&mut adapter)
                .ok_or_else(|| {
                    CodexErr::Fatal("Failed to configure adapter: Arc is shared".into())
                })?
                .configure(&config)?;
            adapter.validate_config(&config)?;
        }

        // Transform request using adapter (no clone needed - context contains config)
        let transformed_request = adapter
            .transform_request(prompt, &context, provider)
            .map_err(|e| {
                CodexErr::Fatal(format!(
                    "Adapter '{adapter_name}' failed to transform request: {e}"
                ))
            })?;

        // Let adapter build dynamic metadata (headers, query params)
        let request_metadata = adapter
            .build_request_metadata(prompt, provider, &context)
            .map_err(|e| {
                CodexErr::Fatal(format!(
                    "Adapter '{adapter_name}' failed to build request metadata: {e}"
                ))
            })?;

        // Use unified streaming method for both Chat and Responses APIs
        self.stream_http(
            transformed_request,
            adapter,
            request_metadata,
            provider,
            global_stream_idle_timeout,
        )
        .await
    }

    /// Unified HTTP streaming for both Chat and Responses APIs
    ///
    /// Consolidates the common HTTP request/response handling logic.
    /// The only difference between Chat and Responses APIs is the endpoint path,
    /// which is determined by the adapter's endpoint_path() or wire_api fallback.
    async fn stream_http(
        &self,
        transformed_request: Value,
        adapter: Arc<dyn ProviderAdapter>,
        request_metadata: RequestMetadata,
        provider: &ModelProviderInfo,
        global_stream_idle_timeout: Option<u64>,
    ) -> Result<ResponseStream> {
        let base_url = provider
            .base_url
            .as_ref()
            .ok_or_else(|| CodexErr::Fatal("Provider base_url is required for adapters".into()))?;

        // Determine endpoint: use adapter's custom path or fallback to wire_api default
        let endpoint = if let Some(path) = adapter.endpoint_path() {
            path.to_string()
        } else {
            match provider.wire_api {
                WireApi::Responses => "/responses".to_string(),
                WireApi::Chat => "/crawl".to_string(),
            }
        };

        // Build URL with query params
        // Start with provider's static query params, then overlay adapter's dynamic params
        let mut combined_params = provider.query_params.clone().unwrap_or_default();
        for (key, value) in &request_metadata.query_params {
            combined_params.insert(key.clone(), value.clone());
        }

        let mut url = format!("{base_url}{endpoint}");
        if !combined_params.is_empty() {
            let query_string = combined_params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{url}?{query_string}");
        }

        // Build request parameters for retry-safe sending
        let api_key = provider.api_key().ok().flatten();

        // Collect all headers (static from provider + dynamic from adapter)
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(provider_headers) = &provider.http_headers {
            for (key, value) in provider_headers {
                headers.push((key.clone(), value.clone()));
            }
        }
        for (key, value) in &request_metadata.headers {
            headers.push((key.clone(), value.clone()));
        }

        let request_params = RequestParams {
            url,
            body: transformed_request,
            headers,
            api_key,
        };

        // Send request with retry support
        let response = self.send_with_retry(request_params, provider).await?;

        // Check status and parse specific error types
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".into());

            // Try to parse as structured error response to detect specific error codes
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&body) {
                let error = &error_response.error;

                // Check for specific error codes and return appropriate CodexErr types
                if is_previous_response_not_found_error(error) {
                    return Err(CodexErr::PreviousResponseNotFound);
                }
                if is_context_window_error(error) {
                    return Err(CodexErr::ContextWindowExceeded);
                }
                if is_quota_exceeded_error(error) {
                    return Err(CodexErr::QuotaExceeded);
                }
            }

            // Fall back to generic Fatal error if not a recognized error code
            return Err(CodexErr::Fatal(format!(
                "Provider returned error {status}: {body}"
            )));
        }

        // Branch based on streaming configuration
        let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(16);

        if provider.ext.streaming {
            // ========== Streaming SSE path (existing logic) ==========
            let byte_stream = response.bytes_stream();
            // Use provider timeout if set, otherwise global timeout, otherwise default
            let idle_timeout = Duration::from_millis(
                provider
                    .stream_idle_timeout_ms
                    .or(global_stream_idle_timeout)
                    .unwrap_or(120_000), // Default 2 minutes
            );
            let provider_arc = Arc::new(provider.clone());
            let otel = self.otel_event_manager.clone();

            tokio::spawn(async move {
                process_sse_with_adapter(
                    byte_stream,
                    tx_event,
                    adapter,
                    provider_arc,
                    idle_timeout,
                    otel,
                )
                .await;
            });
        } else {
            // ========== Non-streaming JSON path (new) ==========
            let provider_arc = Arc::new(provider.clone());

            tokio::spawn(async move {
                match response.text().await {
                    Ok(body) => {
                        let mut ctx = AdapterContext::new();
                        match adapter.transform_response_chunk(&body, &mut ctx, &provider_arc) {
                            Ok(events) => {
                                for event in events {
                                    if tx_event.send(Ok(event)).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx_event.send(Err(e)).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx_event
                            .send(Err(CodexErr::Fatal(format!(
                                "Failed to read response body: {e}"
                            ))))
                            .await;
                    }
                }
            });
        }

        Ok(ResponseStream { rx_event })
    }

    /// Send HTTP request with retry and telemetry support.
    ///
    /// Implements exponential backoff with jitter for retrying failed requests.
    /// Retries on 429 (rate limit), 5xx (server errors), and transport errors.
    ///
    /// Note: Rebuilds the request on each attempt to avoid needing `try_clone()`.
    /// This follows the extension pattern to avoid modifying default_client.rs.
    async fn send_with_retry(
        &self,
        request_params: RequestParams,
        provider: &ModelProviderInfo,
    ) -> Result<reqwest::Response> {
        let policy = build_retry_policy(provider);
        let telemetry = AdapterTelemetry::new(self.otel_event_manager.clone());

        for attempt in 0..=policy.max_attempts {
            let start = Instant::now();

            // Rebuild request on each attempt (avoids try_clone dependency)
            let client = build_reqwest_client();
            let mut builder = client
                .post(&request_params.url)
                .header("content-type", "application/json")
                .json(&request_params.body);

            // Add authentication
            if let Some(ref api_key) = request_params.api_key {
                builder = builder.bearer_auth(api_key);
            }

            // Add headers
            for (key, value) in &request_params.headers {
                builder = builder.header(key.as_str(), value.as_str());
            }

            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    telemetry.on_request(attempt, Some(status), None, start.elapsed());

                    // Check if this is a retryable HTTP error (429 or 5xx)
                    if status.as_u16() == 429 && policy.retry_on.retry_429 {
                        if attempt < policy.max_attempts {
                            tracing::debug!(
                                attempt,
                                status = %status,
                                "Retrying after rate limit"
                            );
                            tokio::time::sleep(backoff(policy.base_delay, attempt + 1)).await;
                            continue;
                        }
                    }
                    if status.is_server_error() && policy.retry_on.retry_5xx {
                        if attempt < policy.max_attempts {
                            tracing::debug!(
                                attempt,
                                status = %status,
                                "Retrying after server error"
                            );
                            tokio::time::sleep(backoff(policy.base_delay, attempt + 1)).await;
                            continue;
                        }
                    }

                    return Ok(resp);
                }
                Err(e) => {
                    let transport_err = TransportError::Network(e.to_string());
                    telemetry.on_request(attempt, None, Some(&transport_err), start.elapsed());

                    if !policy
                        .retry_on
                        .should_retry(&transport_err, attempt, policy.max_attempts)
                    {
                        return Err(map_transport_error(transport_err));
                    }

                    tracing::debug!(
                        attempt,
                        error = %e,
                        "Retrying after transport error"
                    );
                    tokio::time::sleep(backoff(policy.base_delay, attempt + 1)).await;
                }
            }
        }

        Err(CodexErr::RetryLimit(RetryLimitReachedError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            request_id: None,
        }))
    }
}

/// Request parameters for retry-safe request building.
///
/// Contains all the information needed to rebuild a request on each retry attempt.
/// This avoids the need for `try_clone()` which would require modifying default_client.rs.
struct RequestParams {
    url: String,
    body: Value,
    headers: Vec<(String, String)>,
    api_key: Option<String>,
}

/// Process SSE stream using a custom adapter
///
/// Reads SSE events from the byte stream and uses the adapter to transform
/// them into codex-rs ResponseEvents.
async fn process_sse_with_adapter<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent>>,
    adapter: Arc<dyn ProviderAdapter>,
    provider: Arc<crate::model_provider_info::ModelProviderInfo>,
    idle_timeout: Duration,
    _otel_event_manager: OtelEventManager,
) where
    S: Stream<Item = reqwest::Result<Bytes>> + Unpin + Eventsource,
{
    // Create AdapterContext for this request's lifetime.
    //
    // MEMORY MANAGEMENT:
    // - This context exists ONLY for the duration of this function
    // - State accumulates as we process each SSE chunk
    // - When this function returns (normally or on error), context is automatically
    //   dropped and all accumulated state is freed (Rust RAII)
    // - No manual cleanup needed
    // - No memory leaks across requests (each request gets a fresh context)
    //
    // Typical lifecycle:
    //   1. Create context (empty HashMap)
    //   2. Process chunks → state grows (e.g., accumulated text)
    //   3. Request completes → function returns → context drops → memory freed
    let mut adapter_context = AdapterContext::new();
    let mut stream = stream.eventsource();

    loop {
        let response = timeout(idle_timeout, stream.next()).await;

        match response {
            Ok(Some(Ok(sse))) => {
                // Debug: Log received SSE event type
                tracing::debug!(sse_event = %sse.event, "Received SSE event");

                // Skip comments and ping events
                if sse.event == "comment" || sse.event == "ping" {
                    continue;
                }

                // Skip empty data
                if sse.data.trim().is_empty() {
                    tracing::debug!("Skipping empty SSE data");
                    continue;
                }

                // Use adapter to transform the chunk
                match adapter.transform_response_chunk(&sse.data, &mut adapter_context, &provider) {
                    Ok(events) => {
                        for event in events {
                            let event_type = event_type_name(&event);
                            tracing::debug!(event_type, "Sending ResponseEvent");

                            if tx_event.send(Ok(event)).await.is_err() {
                                // Receiver dropped
                                tracing::debug!("Receiver dropped, exiting SSE loop");
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Adapter failed to transform response");
                        let _ = tx_event
                            .send(Err(CodexErr::Fatal(format!(
                                "Adapter failed to transform response: {e}"
                            ))))
                            .await;
                        return;
                    }
                }
            }
            Ok(Some(Err(e))) => {
                tracing::error!(error = %e, "SSE stream error");
                let _ = tx_event
                    .send(Err(CodexErr::Stream(e.to_string(), None)))
                    .await;
                return;
            }
            Ok(None) => {
                // Stream ended normally
                tracing::debug!("SSE stream ended normally");
                return;
            }
            Err(_) => {
                tracing::debug!("SSE stream idle timeout");
                let _ = tx_event
                    .send(Err(CodexErr::Stream("SSE stream timeout".into(), None)))
                    .await;
                return;
            }
        }
    }
}

/// Get event type name for debug logging (without content)
fn event_type_name(event: &ResponseEvent) -> &'static str {
    match event {
        ResponseEvent::Created => "Created",
        ResponseEvent::OutputItemDone(_) => "OutputItemDone",
        ResponseEvent::OutputItemAdded(_) => "OutputItemAdded",
        ResponseEvent::Completed { .. } => "Completed",
        ResponseEvent::OutputTextDelta(_) => "OutputTextDelta",
        ResponseEvent::ReasoningSummaryDelta { .. } => "ReasoningSummaryDelta",
        ResponseEvent::ReasoningContentDelta { .. } => "ReasoningContentDelta",
        ResponseEvent::ReasoningSummaryPartAdded { .. } => "ReasoningSummaryPartAdded",
        ResponseEvent::RateLimits(_) => "RateLimits",
    }
}

/// Check if error is a context window exceeded error
fn is_context_window_error(error: &ErrorDetail) -> bool {
    error.code.as_deref() == Some("context_length_exceeded")
}

/// Check if error is a quota exceeded error
fn is_quota_exceeded_error(error: &ErrorDetail) -> bool {
    error.code.as_deref() == Some("insufficient_quota")
}

/// Check if error is a previous response not found error
fn is_previous_response_not_found_error(error: &ErrorDetail) -> bool {
    error.code.as_deref() == Some("previous_response_not_found")
}
