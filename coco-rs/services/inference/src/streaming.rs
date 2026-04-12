//! Enhanced streaming inference — accumulation, timing, stall detection.
//!
//! TS: services/api/claude.ts — streaming loop with TTFT tracking, stall detection,
//! idle timeouts, and full response collection.

use std::time::{Duration, Instant};

use coco_types::TokenUsage;
use tokio::sync::mpsc;
use tracing::{info, warn};
use vercel_ai_provider::AssistantContentPart;

use crate::stream::StreamEvent;

/// Default idle timeout before aborting a stream (120s).
///
/// TS: STREAM_IDLE_TIMEOUT_MS — kills the stream if no chunks arrive.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Warning threshold for idle periods (60s).
///
/// TS: STREAM_IDLE_WARNING_MS — logs a warning before actual timeout.
const IDLE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

/// Threshold for detecting a streaming stall (30s gap between events).
///
/// TS: STALL_THRESHOLD_MS — gap between events treated as a stall.
const STALL_THRESHOLD: Duration = Duration::from_secs(30);

/// Enhanced streaming wrapper over a StreamEvent channel.
///
/// Provides first-token timestamp tracking, token accumulation,
/// stall detection, and full response collection.
///
/// TS: The streaming loop in query() with ttftMs, stall tracking,
/// and content block accumulation.
pub struct StreamingInference {
    /// Channel receiving stream events.
    rx: mpsc::Receiver<StreamEvent>,
    /// Time the stream was created.
    start_time: Instant,
    /// Time the first content token arrived.
    first_token_time: Option<Instant>,
    /// Accumulated text output.
    accumulated_text: String,
    /// Accumulated reasoning/thinking output.
    accumulated_reasoning: String,
    /// Tool calls being assembled from deltas.
    tool_call_buffers: std::collections::HashMap<String, ToolCallBuffer>,
    /// Final token usage (set when Finish event arrives).
    final_usage: Option<TokenUsage>,
    /// Final stop reason.
    stop_reason: Option<String>,
    /// Whether the stream has finished.
    finished: bool,
    /// Stall detection state.
    stall_tracker: StallTracker,
    /// Idle timeout configuration.
    idle_timeout: Duration,
}

/// Buffer for assembling a tool call from streamed deltas.
#[derive(Debug, Clone)]
pub struct ToolCallBuffer {
    pub id: String,
    pub tool_name: String,
    pub input_json: String,
    pub complete: bool,
}

/// Tracks streaming stalls for diagnostics.
///
/// TS: stallCount, totalStallTime in the streaming loop.
#[derive(Debug, Clone)]
pub struct StallTracker {
    last_event_time: Option<Instant>,
    stall_count: i32,
    total_stall_ms: i64,
}

impl StallTracker {
    fn new() -> Self {
        Self {
            last_event_time: None,
            stall_count: 0,
            total_stall_ms: 0,
        }
    }

    /// Record that an event was received. Returns the stall duration if a stall
    /// was detected (gap > STALL_THRESHOLD since the previous event).
    fn record_event(&mut self) -> Option<Duration> {
        let now = Instant::now();
        let stall = if let Some(last) = self.last_event_time {
            let gap = now.duration_since(last);
            if gap > STALL_THRESHOLD {
                self.stall_count += 1;
                self.total_stall_ms += gap.as_millis() as i64;
                Some(gap)
            } else {
                None
            }
        } else {
            None
        };
        self.last_event_time = Some(now);
        stall
    }

    /// Total number of detected stalls.
    pub fn stall_count(&self) -> i32 {
        self.stall_count
    }

    /// Total stall time in milliseconds.
    pub fn total_stall_ms(&self) -> i64 {
        self.total_stall_ms
    }
}

/// A fully collected response from consuming a stream to completion.
///
/// TS: The accumulated state after the streaming loop completes —
/// text, tool calls, usage, timing.
#[derive(Debug, Clone)]
pub struct CollectedResponse {
    /// Accumulated text output.
    pub text: String,
    /// Accumulated reasoning/thinking output.
    pub reasoning: String,
    /// Completed tool calls.
    pub tool_calls: Vec<ToolCallBuffer>,
    /// Token usage for the request.
    pub usage: TokenUsage,
    /// Stop reason from the model.
    pub stop_reason: String,
    /// Time to first token (milliseconds).
    pub ttft_ms: Option<i64>,
    /// Total stream duration (milliseconds).
    pub total_ms: i64,
    /// Number of streaming stalls detected.
    pub stall_count: i32,
    /// Total stall time (milliseconds).
    pub total_stall_ms: i64,
}

impl StreamingInference {
    /// Create a new streaming inference wrapper.
    pub fn new(rx: mpsc::Receiver<StreamEvent>) -> Self {
        Self {
            rx,
            start_time: Instant::now(),
            first_token_time: None,
            accumulated_text: String::new(),
            accumulated_reasoning: String::new(),
            tool_call_buffers: std::collections::HashMap::new(),
            final_usage: None,
            stop_reason: None,
            finished: false,
            stall_tracker: StallTracker::new(),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Override the idle timeout (for testing or configuration).
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Time to first token in milliseconds, if a token has been received.
    pub fn ttft_ms(&self) -> Option<i64> {
        self.first_token_time
            .map(|t| t.duration_since(self.start_time).as_millis() as i64)
    }

    /// Whether any content token has been received.
    pub fn has_received_token(&self) -> bool {
        self.first_token_time.is_some()
    }

    /// Whether the stream has completed (Finish or Error received).
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Current accumulated text.
    pub fn text(&self) -> &str {
        &self.accumulated_text
    }

    /// Current accumulated reasoning text.
    pub fn reasoning(&self) -> &str {
        &self.accumulated_reasoning
    }

    /// Current stall statistics.
    pub fn stall_tracker(&self) -> &StallTracker {
        &self.stall_tracker
    }

    /// Receive the next event from the stream with idle timeout.
    ///
    /// Returns None if the stream is closed or idle timeout fires.
    /// Tracks TTFT and stalls for each received event.
    ///
    /// TS: The main for-await loop with resetStreamIdleTimer().
    pub async fn next_event(&mut self) -> Option<StreamEvent> {
        if self.finished {
            return None;
        }

        let event = tokio::select! {
            event = self.rx.recv() => event,
            _ = tokio::time::sleep(self.idle_timeout) => {
                warn!(
                    elapsed_ms = self.start_time.elapsed().as_millis() as i64,
                    timeout_ms = self.idle_timeout.as_millis() as i64,
                    "streaming idle timeout — no events received"
                );
                self.finished = true;
                return Some(StreamEvent::Error {
                    message: format!(
                        "streaming idle timeout: no chunks received for {}s",
                        self.idle_timeout.as_secs()
                    ),
                });
            }
        };

        let event = event?;

        // Stall detection
        if let Some(stall_duration) = self.stall_tracker.record_event() {
            warn!(
                stall_secs = stall_duration.as_secs_f64(),
                stall_count = self.stall_tracker.stall_count(),
                total_stall_ms = self.stall_tracker.total_stall_ms(),
                "streaming stall detected"
            );
        }

        // Process event for accumulation
        self.accumulate_event(&event);

        Some(event)
    }

    /// Accumulate state from a stream event.
    fn accumulate_event(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::TextDelta { text } => {
                if self.first_token_time.is_none() {
                    self.first_token_time = Some(Instant::now());
                    info!(ttft_ms = self.ttft_ms().unwrap_or(0), "first token received");
                }
                self.accumulated_text.push_str(text);
            }
            StreamEvent::ReasoningDelta { text } => {
                if self.first_token_time.is_none() {
                    self.first_token_time = Some(Instant::now());
                }
                self.accumulated_reasoning.push_str(text);
            }
            StreamEvent::ToolCallStart { id, tool_name } => {
                if self.first_token_time.is_none() {
                    self.first_token_time = Some(Instant::now());
                }
                self.tool_call_buffers.insert(
                    id.clone(),
                    ToolCallBuffer {
                        id: id.clone(),
                        tool_name: tool_name.clone(),
                        input_json: String::new(),
                        complete: false,
                    },
                );
            }
            StreamEvent::ToolCallDelta { id, delta } => {
                if let Some(buffer) = self.tool_call_buffers.get_mut(id) {
                    buffer.input_json.push_str(delta);
                }
            }
            StreamEvent::ToolCallEnd { id } => {
                if let Some(buffer) = self.tool_call_buffers.get_mut(id) {
                    buffer.complete = true;
                }
            }
            StreamEvent::Finish {
                usage, stop_reason, ..
            } => {
                self.final_usage = Some(*usage);
                self.stop_reason = Some(stop_reason.clone());
                self.finished = true;
            }
            StreamEvent::Error { .. } => {
                self.finished = true;
            }
        }
    }

    /// Consume the entire stream and collect into a complete response.
    ///
    /// TS: The full streaming loop that accumulates contentBlocks, usage,
    /// and timing data into the final response.
    pub async fn collect_full_response(mut self) -> Result<CollectedResponse, StreamingError> {
        while let Some(event) = self.next_event().await {
            if let StreamEvent::Error { message } = &event {
                return Err(StreamingError::StreamError {
                    message: message.clone(),
                });
            }
        }

        let usage = self.final_usage.unwrap_or_default();
        let stop_reason = self.stop_reason.unwrap_or_else(|| "unknown".to_string());
        let total_ms = self.start_time.elapsed().as_millis() as i64;

        let tool_calls: Vec<ToolCallBuffer> = self
            .tool_call_buffers
            .into_values()
            .filter(|b| b.complete)
            .collect();

        Ok(CollectedResponse {
            text: self.accumulated_text,
            reasoning: self.accumulated_reasoning,
            tool_calls,
            usage,
            stop_reason,
            ttft_ms: self.ttft_ms(),
            total_ms,
            stall_count: self.stall_tracker.stall_count(),
            total_stall_ms: self.stall_tracker.total_stall_ms(),
        })
    }
}

/// Errors from streaming inference.
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("stream error: {message}")]
    StreamError { message: String },
    #[error("stream idle timeout")]
    IdleTimeout,
}

#[cfg(test)]
#[path = "streaming.test.rs"]
mod tests;
