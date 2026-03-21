//! Stream processor for Crush-like message accumulation.
//!
//! This module provides [`StreamProcessor`], a high-level API for processing
//! streaming responses with accumulated state. The design is inspired by
//! Crush's message update pattern where a single message is continuously
//! updated during streaming, enabling real-time UI updates while maintaining
//! a single aggregated message in history.
//!
//! # Key Features
//!
//! - **Accumulated State**: Access the current snapshot at any time during streaming
//! - **Closure-based API**: No need to implement traits, just pass closures
//! - **Async Native**: All handlers support async operations (DB, WebSocket, etc.)
//! - **Progressive Complexity**: Simple use cases are simple, complex ones are possible
//!
//! # Examples
//!
//! ## Simplest: Collect to response
//!
//! ```ignore
//! let response = model.stream(request).await?.into_processor().collect().await?;
//! ```
//!
//! ## Print to stdout
//!
//! ```ignore
//! let response = model.stream(request).await?.into_processor().print().await?;
//! ```
//!
//! ## Crush-like pattern: Update same message
//!
//! ```ignore
//! let msg_id = db.insert_message(conv_id, Role::Assistant).await?;
//!
//! let response = model.stream(request).await?.into_processor()
//!     .on_update(|snapshot| async move {
//!         // UPDATE same message (not INSERT)
//!         db.update_message(msg_id, &snapshot.text).await?;
//!         // Notify UI subscribers
//!         pubsub.publish(format!("message:{}", msg_id), "updated").await;
//!         Ok(())
//!     })
//!     .await?;
//! ```

use super::EventStream;
use super::StreamEvent;
use super::processor_state::ProcessorState;
use super::response::StreamConfig;
use super::snapshot::StreamSnapshot;
use super::update::StreamUpdate;
use crate::error::HyperError;
use crate::messages::ContentBlock;
use crate::response::FinishReason;
use crate::response::GenerateResponse;
use futures::StreamExt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::timeout;

/// Stream processor with Crush-like accumulated state.
///
/// This is the main type for processing streaming responses. It wraps an
/// [`EventStream`] and maintains accumulated state that can be accessed
/// at any time during streaming.
///
/// # Design
///
/// The processor accumulates all events into a [`StreamSnapshot`] which
/// represents the current state of the response. This enables the
/// "update same message" pattern used by Crush and similar systems.
///
/// # Idle Timeout
///
/// The processor includes an idle timeout (default 60 seconds) to prevent
/// hanging on unresponsive streams. Use [`idle_timeout`](Self::idle_timeout)
/// to customize this behavior.
pub struct StreamProcessor {
    inner: EventStream,
    state: ProcessorState,
    config: StreamConfig,
}

impl StreamProcessor {
    /// Create a new processor from an event stream.
    pub fn new(inner: EventStream) -> Self {
        Self {
            inner,
            state: ProcessorState::default(),
            config: StreamConfig::default(),
        }
    }

    /// Create a new processor with custom configuration.
    pub fn with_config(inner: EventStream, config: StreamConfig) -> Self {
        Self {
            inner,
            state: ProcessorState::default(),
            config,
        }
    }

    /// Set the idle timeout for the processor.
    ///
    /// This is a builder method that allows chaining.
    pub fn idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.config.idle_timeout = idle_timeout;
        self
    }

    /// Get the current configuration.
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    // =========================================================================
    // Low-level API: Iterator style
    // =========================================================================

    /// Get the next raw event from the stream.
    ///
    /// This is the lowest-level API. Events are NOT automatically accumulated
    /// into the snapshot when using this method.
    ///
    /// Respects the configured idle timeout.
    pub async fn next_raw_event(&mut self) -> Option<Result<StreamEvent, HyperError>> {
        match timeout(self.config.idle_timeout, self.inner.next()).await {
            Ok(Some(event)) => Some(event),
            Ok(None) => None,
            Err(_) => Some(Err(HyperError::StreamIdleTimeout(self.config.idle_timeout))),
        }
    }

    /// Get the next event, update the snapshot, and return both.
    ///
    /// Returns the update event along with a clone of the current accumulated snapshot.
    /// Respects the configured idle timeout.
    pub async fn next(&mut self) -> Option<Result<(StreamUpdate, StreamSnapshot), HyperError>> {
        let result = timeout(self.config.idle_timeout, self.inner.next()).await;

        match result {
            Ok(Some(Ok(ev))) => {
                let update: StreamUpdate = ev.clone().into();
                self.update_state(&ev);
                Some(Ok((update, self.state.snapshot.clone())))
            }
            Ok(Some(Err(e))) => Some(Err(e)),
            Ok(None) => None,
            Err(_) => Some(Err(HyperError::StreamIdleTimeout(self.config.idle_timeout))),
        }
    }

    /// Get the current accumulated snapshot.
    ///
    /// Can be called at any time to get the current state.
    pub fn snapshot(&self) -> &StreamSnapshot {
        &self.state.snapshot
    }

    /// Clone the current snapshot.
    ///
    /// Useful when you need to capture the state at a specific point.
    pub fn snapshot_clone(&self) -> StreamSnapshot {
        self.state.snapshot.clone()
    }

    // =========================================================================
    // High-level API: Closure style (recommended)
    // =========================================================================

    /// Process the stream, calling the handler after each update with the accumulated snapshot.
    ///
    /// This is the core API for Crush-like patterns. The handler receives a clone of
    /// the current accumulated state after each event, enabling "UPDATE same message"
    /// patterns.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = processor
    ///     .on_update(|snapshot| async move {
    ///         db.update_message(msg_id, &snapshot.text).await?;
    ///         Ok(())
    ///     })
    ///     .await?;
    /// ```
    #[must_use = "this returns a Result that must be handled"]
    pub async fn on_update<F, Fut>(mut self, mut handler: F) -> Result<GenerateResponse, HyperError>
    where
        F: FnMut(StreamSnapshot) -> Fut,
        Fut: Future<Output = Result<(), HyperError>>,
    {
        while let Some(result) = self.next().await {
            let (_, snapshot) = result?;
            handler(snapshot).await?;
        }
        self.into_response()
    }

    /// Process the stream, calling the handler with both the update event and snapshot.
    ///
    /// Use this when you need to distinguish between event types while also
    /// having access to the accumulated state.
    ///
    /// # Example
    ///
    /// ```ignore
    /// processor.for_each(|update, snapshot| async move {
    ///     match update {
    ///         StreamUpdate::TextDelta { delta, .. } => {
    ///             ui.append_text(&delta);
    ///         }
    ///         StreamUpdate::Done { .. } => {
    ///             ui.mark_complete();
    ///         }
    ///         _ => {}
    ///     }
    ///     // Also update status with accumulated stats
    ///     ui.set_status(&format!("Chars: {}", snapshot.text.len()));
    ///     Ok(())
    /// }).await?;
    /// ```
    #[must_use = "this returns a Result that must be handled"]
    pub async fn for_each<F, Fut>(mut self, mut handler: F) -> Result<GenerateResponse, HyperError>
    where
        F: FnMut(StreamUpdate, StreamSnapshot) -> Fut,
        Fut: Future<Output = Result<(), HyperError>>,
    {
        while let Some(result) = self.next().await {
            let (update, snapshot) = result?;
            handler(update, snapshot).await?;
        }
        self.into_response()
    }

    /// Process only text deltas.
    ///
    /// Simple API for just handling text output (e.g., printing to console).
    ///
    /// # Example
    ///
    /// ```ignore
    /// processor.on_text(|delta| async move {
    ///     print!("{}", delta);
    ///     std::io::stdout().flush()?;
    ///     Ok(())
    /// }).await?;
    /// ```
    #[must_use = "this returns a Result that must be handled"]
    pub async fn on_text<F, Fut>(mut self, mut handler: F) -> Result<GenerateResponse, HyperError>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<(), HyperError>>,
    {
        while let Some(result) = self.next().await {
            let (update, _) = result?;
            if let Some(delta) = update.as_text_delta() {
                handler(delta.to_string()).await?;
            }
        }
        self.into_response()
    }

    /// Process text deltas with accumulated text.
    ///
    /// Like `on_text`, but also receives the full accumulated text so far.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn on_text_with_full<F, Fut>(
        mut self,
        mut handler: F,
    ) -> Result<GenerateResponse, HyperError>
    where
        F: FnMut(String, String) -> Fut,
        Fut: Future<Output = Result<(), HyperError>>,
    {
        while let Some(result) = self.next().await {
            let (update, snapshot) = result?;
            if let Some(delta) = update.as_text_delta() {
                handler(delta.to_string(), snapshot.text).await?;
            }
        }
        self.into_response()
    }

    // =========================================================================
    // Convenience API: One-liners
    // =========================================================================

    /// Silently consume the stream and return the final response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = processor.collect().await?;
    /// println!("{}", response.text());
    /// ```
    #[must_use = "this returns a Result that must be handled"]
    pub async fn collect(mut self) -> Result<GenerateResponse, HyperError> {
        while let Some(result) = self.next().await {
            result?;
        }
        self.into_response()
    }

    /// Print text deltas to stdout and return the final response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = processor.print().await?;
    /// ```
    #[must_use = "this returns a Result that must be handled"]
    pub async fn print(self) -> Result<GenerateResponse, HyperError> {
        self.on_text(|delta| {
            let fut: Pin<Box<dyn Future<Output = Result<(), HyperError>> + Send>> =
                Box::pin(async move {
                    print!("{delta}");
                    Ok(())
                });
            fut
        })
        .await
    }

    /// Print text deltas to stdout with a final newline.
    #[must_use = "this returns a Result that must be handled"]
    pub async fn println(self) -> Result<GenerateResponse, HyperError> {
        let response = self.print().await?;
        println!();
        Ok(response)
    }

    // =========================================================================
    // Internal: State management (delegated to processor_state module)
    // =========================================================================

    fn update_state(&mut self, event: &StreamEvent) {
        self.state.update(event);
    }

    /// Convert the accumulated state into a GenerateResponse.
    #[must_use = "this returns a Result that must be handled"]
    pub fn into_response(self) -> Result<GenerateResponse, HyperError> {
        let snapshot = self.state.snapshot;

        let mut content = Vec::new();

        // Add thinking if present
        if let Some(thinking) = &snapshot.thinking {
            content.push(ContentBlock::Thinking {
                content: thinking.content.clone(),
                signature: thinking.signature.clone(),
            });
        }

        // Add text if present
        if !snapshot.text.is_empty() {
            content.push(ContentBlock::text(&snapshot.text));
        }

        // Add tool calls
        for tc in &snapshot.tool_calls {
            if tc.is_complete {
                content.push(ContentBlock::tool_use(
                    &tc.id,
                    &tc.name,
                    tc.parsed_arguments().unwrap_or(serde_json::Value::Null),
                ));
            }
        }

        Ok(GenerateResponse {
            id: snapshot.id.unwrap_or_else(|| "unknown".to_string()),
            content,
            finish_reason: snapshot.finish_reason.unwrap_or(FinishReason::Stop),
            usage: snapshot.usage,
            model: if snapshot.model.is_empty() {
                "unknown".to_string()
            } else {
                snapshot.model
            },
        })
    }
}

impl std::fmt::Debug for StreamProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamProcessor")
            .field("snapshot", &self.state.snapshot)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "processor.test.rs"]
mod tests;
