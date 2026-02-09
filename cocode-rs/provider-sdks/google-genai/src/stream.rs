//! Streaming support for Google Generative AI API.
//!
//! This module provides SSE (Server-Sent Events) parsing for streaming responses
//! from the `streamGenerateContent` endpoint.
//!
//! ## SSE Specification
//!
//! This implementation follows the [SSE specification](https://html.spec.whatwg.org/multipage/server-sent-events.html):
//! - Lines starting with `:` are comments (ignored)
//! - Fields: `event`, `data`, `id`, `retry`
//! - Multiple `data:` lines are joined with `\n`
//! - Empty line triggers event emission
//! - `id` persists across events (per spec)

use crate::error::GenAiError;
use crate::error::Result;
use crate::types::ErrorResponse;
use crate::types::GenerateContentResponse;
use bytes::Bytes;
use futures::stream::Stream;
use serde::de::DeserializeOwned;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

// =============================================================================
// Type Aliases
// =============================================================================

/// Type alias for a streaming response of raw SSE events.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ServerSentEvent>> + Send>>;

/// Type alias for a streaming response of parsed GenerateContentResponse chunks.
///
/// Each item in the stream is a `GenerateContentResponse` chunk containing
/// partial content that should be accumulated by the caller.
pub type ContentStream = Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>;

/// Type alias for a boxed byte stream (pinned for polling).
type BoxedByteStream =
    Pin<Box<dyn Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send>>;

// =============================================================================
// ServerSentEvent
// =============================================================================

/// A parsed Server-Sent Event following the SSE specification.
///
/// This aligns with Python SDK's `ServerSentEvent` class.
///
/// ## SSE Wire Format
///
/// ```text
/// event: message
/// data: {"key": "value"}
/// id: 123
/// retry: 5000
///
/// ```
#[derive(Debug, Clone, Default)]
pub struct ServerSentEvent {
    /// Event type (from "event:" field).
    pub event: Option<String>,
    /// Event data (from "data:" fields, joined with newlines for multi-line data).
    pub data: String,
    /// Event ID (from "id:" field). Persists across events per SSE spec.
    pub id: Option<String>,
    /// Retry timeout in milliseconds (from "retry:" field).
    pub retry: Option<i32>,
}

impl ServerSentEvent {
    /// Create a new empty SSE.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new SSE with data.
    pub fn with_data(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            ..Default::default()
        }
    }

    /// Check if this event has non-empty data.
    pub fn has_data(&self) -> bool {
        !self.data.is_empty()
    }

    /// Check if this is the [DONE] marker.
    pub fn is_done(&self) -> bool {
        self.data.starts_with("[DONE]")
    }

    /// Parse the data as JSON.
    ///
    /// # Errors
    ///
    /// Returns `GenAiError::Parse` if the data is not valid JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.data).map_err(|e| {
            GenAiError::Parse(format!(
                "Failed to parse SSE data as JSON: {e}\nData: {}",
                self.data
            ))
        })
    }
}

// =============================================================================
// SSEDecoder
// =============================================================================

/// Server-Sent Events decoder following the full SSE specification.
///
/// This aligns with Python SDK's `SSEDecoder` class.
///
/// ## Implementation Notes
///
/// Per the [SSE specification](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation):
/// - Lines starting with `:` are comments (ignored)
/// - Supported fields: `event`, `data`, `id`, `retry`
/// - Unknown fields are ignored
/// - Multiple `data:` lines are joined with `\n`
/// - Empty line triggers event emission
/// - `id` persists across events (do NOT reset on event emission)
/// - `id` containing null character (`\0`) is ignored
#[derive(Debug, Default)]
pub struct SSEDecoder {
    /// Current event type (reset on event emission).
    event: Option<String>,
    /// Accumulated data lines (reset on event emission).
    data: Vec<String>,
    /// Last event ID (persists across events per SSE spec).
    last_event_id: Option<String>,
    /// Retry timeout (reset on event emission).
    retry: Option<i32>,
}

impl SSEDecoder {
    /// Create a new SSE decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode a single line of SSE data.
    ///
    /// Returns `Some(ServerSentEvent)` when a complete event is ready (empty line received).
    /// Returns `None` if more data is needed.
    ///
    /// # SSE Line Format
    ///
    /// ```text
    /// field:value
    /// field: value  (space after colon is stripped)
    /// :comment      (ignored)
    /// ```
    pub fn decode(&mut self, line: &str) -> Option<ServerSentEvent> {
        // Empty line = emit event
        if line.is_empty() {
            // Don't emit empty event
            if self.event.is_none() && self.data.is_empty() && self.retry.is_none() {
                return None;
            }

            let event = ServerSentEvent {
                event: self.event.take(),
                data: self.data.join("\n"),
                id: self.last_event_id.clone(), // Persists per SSE spec
                retry: self.retry.take(),
            };

            self.data.clear();
            return Some(event);
        }

        // Comment line (starts with ':')
        if line.starts_with(':') {
            return None;
        }

        // Parse field:value
        let (field, value) = if let Some(colon_pos) = line.find(':') {
            let field = &line[..colon_pos];
            let mut value = &line[colon_pos + 1..];
            // Strip single leading space after colon per spec
            if value.starts_with(' ') {
                value = &value[1..];
            }
            (field, value)
        } else {
            // Line with no colon - treat entire line as field name with empty value
            (line, "")
        };

        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            "id" => {
                // Ignore IDs containing null character per SSE spec
                if !value.contains('\0') {
                    self.last_event_id = Some(value.to_string());
                }
            }
            "retry" => {
                if let Ok(ms) = value.parse::<i32>() {
                    self.retry = Some(ms);
                }
                // Invalid retry values are ignored per spec
            }
            _ => {} // Unknown field, ignore per spec
        }

        None
    }

    /// Process a chunk of bytes and yield complete events.
    ///
    /// This handles incomplete lines across chunk boundaries.
    pub fn decode_chunk(&mut self, chunk: &[u8], buffer: &mut Vec<u8>) -> Vec<ServerSentEvent> {
        buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        // Process complete lines
        loop {
            if let Some(line_info) = find_line_end(buffer) {
                // Extract line bytes
                let line_bytes: Vec<u8> = buffer.drain(..line_info.end).collect();
                // Remove the line ending
                buffer.drain(..line_info.ending_len);

                // Decode line as UTF-8
                if let Ok(line) = std::str::from_utf8(&line_bytes) {
                    if let Some(event) = self.decode(line) {
                        events.push(event);
                    }
                }
            } else {
                break;
            }
        }

        events
    }

    /// Reset the decoder state.
    pub fn reset(&mut self) {
        self.event = None;
        self.data.clear();
        self.last_event_id = None;
        self.retry = None;
    }
}

/// Information about a line ending found in a buffer.
struct LineEnd {
    /// Position of line content end (before line ending).
    end: usize,
    /// Length of line ending characters to skip.
    ending_len: usize,
}

/// Find the end of the next line in the buffer.
///
/// Handles `\n`, `\r`, and `\r\n` line endings per SSE spec.
fn find_line_end(buffer: &[u8]) -> Option<LineEnd> {
    for (i, &byte) in buffer.iter().enumerate() {
        if byte == b'\n' {
            return Some(LineEnd {
                end: i,
                ending_len: 1,
            });
        }
        if byte == b'\r' {
            // Check for \r\n
            let ending_len = if buffer.get(i + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            return Some(LineEnd { end: i, ending_len });
        }
    }
    None
}

// =============================================================================
// SseStream - Low-level SSE byte stream to event stream
// =============================================================================

/// SSE parser that converts a byte stream into SSE events.
///
/// Implements the `Stream` trait for async iteration.
pub struct SseStream {
    inner: BoxedByteStream,
    decoder: SSEDecoder,
    buffer: Vec<u8>,
    pending_events: Vec<ServerSentEvent>,
}

impl SseStream {
    fn new(inner: BoxedByteStream) -> Self {
        Self {
            inner,
            decoder: SSEDecoder::new(),
            buffer: Vec::new(),
            pending_events: Vec::new(),
        }
    }
}

impl Stream for SseStream {
    type Item = Result<ServerSentEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Return pending events first
            if !self.pending_events.is_empty() {
                return Poll::Ready(Some(Ok(self.pending_events.remove(0))));
            }

            // Poll inner stream for more data
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    // Decode chunk and collect events
                    // Temporarily take buffer to avoid borrow issues
                    let mut buffer = std::mem::take(&mut self.buffer);
                    let events = self.decoder.decode_chunk(&bytes, &mut buffer);
                    self.buffer = buffer;
                    self.pending_events.extend(events);
                    // Continue loop to return first event
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(GenAiError::Network(e.to_string()))));
                }
                Poll::Ready(None) => {
                    // Stream ended - try to parse remaining buffer
                    if !self.buffer.is_empty() {
                        // Take buffer to avoid borrow issues
                        let buffer = std::mem::take(&mut self.buffer);
                        if let Ok(remaining) = std::str::from_utf8(&buffer) {
                            // Try decoding remaining as final line
                            if let Some(event) = self.decoder.decode(remaining) {
                                return Poll::Ready(Some(Ok(event)));
                            }
                            // Try triggering final event with empty line
                            if let Some(event) = self.decoder.decode("") {
                                return Poll::Ready(Some(Ok(event)));
                            }
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// =============================================================================
// ContentStreamParser - SSE to GenerateContentResponse
// =============================================================================

/// Parser that converts SSE events into GenerateContentResponse chunks.
struct ContentStreamParser {
    inner: SseStream,
}

impl ContentStreamParser {
    fn new(inner: SseStream) -> Self {
        Self { inner }
    }
}

impl Stream for ContentStreamParser {
    type Item = Result<GenerateContentResponse>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(sse))) => {
                    // Skip empty data and [DONE] marker
                    if !sse.has_data() || sse.is_done() {
                        continue;
                    }

                    // First check if this is an error response
                    // (Aligns with Python SDK's error handling in streams)
                    if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&sse.data) {
                        return Poll::Ready(Some(Err(GenAiError::Api {
                            code: error_response.error.code,
                            message: error_response.error.message,
                            status: error_response.error.status,
                        })));
                    }

                    // Parse as GenerateContentResponse
                    match sse.json::<GenerateContentResponse>() {
                        Ok(response) => return Poll::Ready(Some(Ok(response))),
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    }
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Parse an SSE byte stream into a stream of raw `ServerSentEvent`.
///
/// Use this for low-level SSE event access.
pub fn parse_sse_events<S>(byte_stream: S) -> EventStream
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let boxed: BoxedByteStream = Box::pin(byte_stream);
    Box::pin(SseStream::new(boxed))
}

/// Parse an SSE byte stream into a stream of `GenerateContentResponse` chunks.
///
/// This is the main entry point for streaming content generation.
///
/// # SSE Wire Format
///
/// The Google Gemini streaming API uses Server-Sent Events format:
/// ```text
/// data: {"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}
///
/// data: {"candidates":[{"content":{"parts":[{"text":" World"}]}}]}
///
/// data: [DONE]
/// ```
///
/// Each `data:` line contains a complete JSON `GenerateContentResponse`.
pub fn parse_sse_stream<S>(byte_stream: S) -> ContentStream
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let boxed: BoxedByteStream = Box::pin(byte_stream);
    Box::pin(ContentStreamParser::new(SseStream::new(boxed)))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[path = "stream.test.rs"]
mod tests;
