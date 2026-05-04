//! StreamProcessor: mid-level API for consuming streaming responses.

use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
use futures::StreamExt;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::errors::AISdkError;
use vercel_ai_provider::language_model::v4::stream::LanguageModelV4StreamPart;

use super::metrics::StreamMetrics;
use super::metrics::StreamMetricsTracker;
use super::processor_state::ProcessorState;
use super::snapshot::StreamSnapshot;

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_STALL_THRESHOLD: Duration = Duration::from_secs(30);

/// Configuration for the stream processor.
#[derive(Debug, Clone)]
pub struct StreamProcessorConfig {
    /// Timeout for idle streams (no events received). `None` disables idle
    /// timeout checks. Default: 60s.
    pub idle_timeout: Option<Duration>,
    /// Gap between stream items that is counted as a stall. Default: 30s.
    pub stall_threshold: Duration,
}

impl Default for StreamProcessorConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Some(DEFAULT_IDLE_TIMEOUT),
            stall_threshold: DEFAULT_STALL_THRESHOLD,
        }
    }
}

impl StreamProcessorConfig {
    /// Set the idle timeout.
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    /// Disable idle timeout checks.
    pub fn without_idle_timeout(mut self) -> Self {
        self.idle_timeout = None;
        self
    }

    /// Set the stall detection threshold.
    pub fn with_stall_threshold(mut self, threshold: Duration) -> Self {
        self.stall_threshold = threshold;
        self
    }
}

/// Processes a vercel-ai stream into accumulated snapshots.
///
/// Three API levels:
///   1. `.next()` → `(StreamPart, &StreamSnapshot)` — low-level, event-by-event
///   2. `.on_update(|snapshot| ...)` — callback-based processing
///   3. `.collect()` / `.into_text()` — convenience for collecting all events
pub struct StreamProcessor {
    stream:
        Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send + 'static>>,
    config: StreamProcessorConfig,
    state: ProcessorState,
    metrics: StreamMetricsTracker,
}

impl StreamProcessor {
    /// Create a new StreamProcessor from a stream result.
    pub fn new(result: LanguageModelV4StreamResult) -> Self {
        Self::new_with_config(result, StreamProcessorConfig::default())
    }

    /// Create a new StreamProcessor from a stream result and explicit config.
    pub fn new_with_config(
        result: LanguageModelV4StreamResult,
        config: StreamProcessorConfig,
    ) -> Self {
        Self::from_stream_with_config(result.stream, config)
    }

    /// Create a StreamProcessor from a raw stream.
    pub fn from_stream(
        stream: Pin<
            Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send + 'static>,
        >,
    ) -> Self {
        Self::from_stream_with_config(stream, StreamProcessorConfig::default())
    }

    /// Create a StreamProcessor from a raw stream and explicit config.
    pub fn from_stream_with_config(
        stream: Pin<
            Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send + 'static>,
        >,
        config: StreamProcessorConfig,
    ) -> Self {
        let stall_threshold = config.stall_threshold;
        Self {
            stream,
            config,
            state: ProcessorState::new(),
            metrics: StreamMetricsTracker::new(stall_threshold),
        }
    }

    /// Set the idle timeout.
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.with_idle_timeout(timeout);
        self
    }

    /// Disable idle timeout checks for this processor.
    pub fn disable_idle_timeout(mut self) -> Self {
        self.config = self.config.without_idle_timeout();
        self
    }

    /// Set the stall detection threshold.
    pub fn stall_threshold(mut self, threshold: Duration) -> Self {
        self.config = self.config.with_stall_threshold(threshold);
        self.metrics.set_stall_threshold(threshold);
        self
    }

    /// Get a reference to the current snapshot.
    pub fn snapshot(&self) -> &StreamSnapshot {
        &self.state.snapshot
    }

    /// Get the current stream health metrics.
    pub fn metrics(&self) -> StreamMetrics {
        self.metrics.snapshot()
    }

    /// Pull next event from the stream, updating internal snapshot.
    ///
    /// Returns `None` when the stream ends. Each call yields the raw event
    /// along with a reference to the updated snapshot.
    pub async fn next(
        &mut self,
    ) -> Option<Result<(LanguageModelV4StreamPart, &StreamSnapshot), AISdkError>> {
        let next_item = if let Some(timeout) = self.config.idle_timeout {
            match tokio::time::timeout(timeout, self.stream.next()).await {
                Ok(item) => item,
                Err(_) => {
                    return Some(Err(AISdkError::new(format!(
                        "Stream idle timeout after {}s",
                        timeout.as_secs()
                    ))));
                }
            }
        } else {
            self.stream.next().await
        };

        match next_item {
            Some(Ok(part)) => {
                self.metrics.record_item(Some(&part));
                self.state.update(&part);
                Some(Ok((part, &self.state.snapshot)))
            }
            Some(Err(e)) => {
                self.metrics.record_item(None);
                Some(Err(e))
            }
            None => None,
        }
    }

    /// Collect all events into a final snapshot.
    pub async fn collect(mut self) -> Result<StreamSnapshot, AISdkError> {
        while let Some(result) = self.next().await {
            result?;
        }
        Ok(self.state.snapshot)
    }

    /// Collect and return only the accumulated text.
    pub async fn into_text(self) -> Result<String, AISdkError> {
        let snapshot = self.collect().await?;
        Ok(snapshot.text)
    }
}

impl std::fmt::Debug for StreamProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamProcessor")
            .field("config", &self.config)
            .field("is_complete", &self.state.snapshot.is_complete)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "processor.test.rs"]
mod tests;
