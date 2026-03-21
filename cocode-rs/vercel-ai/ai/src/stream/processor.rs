//! StreamProcessor: mid-level API for consuming streaming responses.

use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
use futures::StreamExt;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::errors::AISdkError;
use vercel_ai_provider::language_model::v4::stream::LanguageModelV4StreamPart;

use super::processor_state::ProcessorState;
use super::snapshot::StreamSnapshot;

/// Configuration for the stream processor.
#[derive(Debug, Clone)]
pub struct StreamProcessorConfig {
    /// Timeout for idle streams (no events received). Default: 60s.
    pub idle_timeout: Duration,
}

impl Default for StreamProcessorConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(60),
        }
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
}

impl StreamProcessor {
    /// Create a new StreamProcessor from a stream result.
    pub fn new(result: LanguageModelV4StreamResult) -> Self {
        Self {
            stream: result.stream,
            config: StreamProcessorConfig::default(),
            state: ProcessorState::new(),
        }
    }

    /// Create a StreamProcessor from a raw stream.
    pub fn from_stream(
        stream: Pin<
            Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send + 'static>,
        >,
    ) -> Self {
        Self {
            stream,
            config: StreamProcessorConfig::default(),
            state: ProcessorState::new(),
        }
    }

    /// Set the idle timeout.
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.config.idle_timeout = timeout;
        self
    }

    /// Get a reference to the current snapshot.
    pub fn snapshot(&self) -> &StreamSnapshot {
        &self.state.snapshot
    }

    /// Pull next event from the stream, updating internal snapshot.
    ///
    /// Returns `None` when the stream ends. Each call yields the raw event
    /// along with a reference to the updated snapshot.
    pub async fn next(
        &mut self,
    ) -> Option<Result<(LanguageModelV4StreamPart, &StreamSnapshot), AISdkError>> {
        let timeout = self.config.idle_timeout;

        let result = tokio::time::timeout(timeout, self.stream.next()).await;

        match result {
            Ok(Some(Ok(part))) => {
                self.state.update(&part);
                Some(Ok((part, &self.state.snapshot)))
            }
            Ok(Some(Err(e))) => Some(Err(e)),
            Ok(None) => None,
            Err(_) => Some(Err(AISdkError::new(format!(
                "Stream idle timeout after {}s",
                timeout.as_secs()
            )))),
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
