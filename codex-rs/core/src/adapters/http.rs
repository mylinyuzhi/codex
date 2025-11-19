use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::timeout;

use codex_otel::otel_event_manager::OtelEventManager;
use codex_protocol::ConversationId;
use codex_protocol::config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::protocol::SessionSource;

use crate::adapters::AdapterConfig;
use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestMetadata;
use crate::adapters::get_adapter;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::default_client::CodexHttpClient;
use crate::error::CodexErr;
use crate::error::Result;
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

/// HTTP client for adapter-based provider communication
///
/// Handles HTTP request/response transformation and streaming for providers
/// that use custom adapters (non-OpenAI providers).
pub(crate) struct AdapterHttpClient {
    http_client: CodexHttpClient,
    otel_event_manager: OtelEventManager,
}

impl AdapterHttpClient {
    pub fn new(http_client: CodexHttpClient, otel_event_manager: OtelEventManager) -> Self {
        Self {
            http_client,
            otel_event_manager,
        }
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
        provider: &ModelProviderInfo,
        adapter_name: &str,
        conversation_id: ConversationId,
        session_source: SessionSource,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        global_stream_idle_timeout: Option<u64>,
    ) -> Result<ResponseStream> {
        // Get adapter from registry
        let mut adapter = get_adapter(adapter_name)
            .map_err(|e| CodexErr::Fatal(format!("Failed to get adapter '{adapter_name}': {e}")))?;

        // Configure adapter if provider has adapter_config
        if let Some(config_map) = &provider.adapter_config {
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

        // Clone prompt and inject reasoning configuration
        let mut enhanced_prompt = prompt.clone();
        enhanced_prompt.reasoning_effort = effort;
        enhanced_prompt.reasoning_summary = Some(summary);

        // Transform request using adapter
        let transformed_request = adapter
            .transform_request(&enhanced_prompt, provider)
            .map_err(|e| {
                CodexErr::Fatal(format!(
                    "Adapter '{adapter_name}' failed to transform request: {e}"
                ))
            })?;

        // Build runtime context for dynamic headers/params
        let request_context = crate::adapters::RequestContext {
            conversation_id: conversation_id.to_string(),
            session_source: format!("{session_source:?}"),
        };

        // Let adapter build dynamic metadata (headers, query params)
        let request_metadata = adapter
            .build_request_metadata(&enhanced_prompt, provider, &request_context)
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
                WireApi::Chat => "/chat/completions".to_string(),
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

        // Build HTTP request
        let mut request_builder = self
            .http_client
            .post(&url)
            .header("content-type", "application/json")
            .json(&transformed_request);

        // Add authentication
        if let Ok(Some(api_key)) = provider.api_key() {
            request_builder = request_builder.bearer_auth(api_key);
        }

        // Add static headers from provider config
        if let Some(headers) = &provider.http_headers {
            for (key, value) in headers {
                request_builder = request_builder.header(key, value);
            }
        }

        // Add dynamic headers from adapter
        for (key, value) in &request_metadata.headers {
            request_builder = request_builder.header(key, value);
        }

        // Send request
        let response = request_builder
            .send()
            .await
            .map_err(|e| CodexErr::Fatal(format!("Failed to connect to provider: {e}")))?;

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

        if provider.streaming {
            // ========== Streaming SSE path (existing logic) ==========
            let byte_stream = response.bytes_stream();
            let idle_timeout = provider.effective_stream_idle_timeout(global_stream_idle_timeout);
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
    S: Stream<Item = reqwest::Result<Bytes>> + Unpin,
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
        ResponseEvent::ReasoningSummaryDelta(_) => "ReasoningSummaryDelta",
        ResponseEvent::ReasoningContentDelta(_) => "ReasoningContentDelta",
        ResponseEvent::ReasoningSummaryPartAdded => "ReasoningSummaryPartAdded",
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
