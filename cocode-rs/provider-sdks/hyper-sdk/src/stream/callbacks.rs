//! Callback-based stream processing.

use super::events::StreamEvent;
use super::response::StreamResponse;
use crate::error::HyperError;
use crate::response::FinishReason;
use crate::response::GenerateResponse;
use crate::tools::ToolCall;
use async_trait::async_trait;

/// Callbacks for stream events.
///
/// Implement this trait to handle streaming events as they arrive.
/// All methods have default no-op implementations, so you only need
/// to implement the ones you care about.
#[async_trait]
pub trait StreamCallbacks: Send {
    /// Called when a text delta is received.
    async fn on_text_delta(&mut self, _index: i64, _delta: &str) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when a text block is complete.
    async fn on_text_done(&mut self, _index: i64, _text: &str) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when a thinking delta is received.
    async fn on_thinking_delta(&mut self, _index: i64, _delta: &str) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when a thinking block is complete.
    async fn on_thinking_done(&mut self, _index: i64, _content: &str) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when a tool call starts.
    async fn on_tool_call_start(
        &mut self,
        _index: i64,
        _id: &str,
        _name: &str,
    ) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when a tool call is complete.
    async fn on_tool_call_done(
        &mut self,
        _index: i64,
        _tool_call: &ToolCall,
    ) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when the response is complete.
    async fn on_finish(&mut self, _reason: FinishReason) -> Result<(), HyperError> {
        Ok(())
    }

    /// Called when an error occurs.
    async fn on_error(&mut self, _error: &HyperError) -> Result<(), HyperError> {
        Ok(())
    }
}

impl StreamResponse {
    /// Process the stream with callbacks.
    ///
    /// Consumes the stream and calls the appropriate callback for each event.
    /// Returns the final response when complete.
    pub async fn process_with_callbacks<C: StreamCallbacks>(
        mut self,
        mut callbacks: C,
    ) -> Result<GenerateResponse, HyperError> {
        while let Some(result) = self.next_event().await {
            match result {
                Ok(event) => {
                    Self::dispatch_event(&mut callbacks, &event).await?;
                }
                Err(e) => {
                    callbacks.on_error(&e).await?;
                    return Err(e);
                }
            }
        }
        self.build_response()
    }

    async fn dispatch_event<C: StreamCallbacks>(
        callbacks: &mut C,
        event: &StreamEvent,
    ) -> Result<(), HyperError> {
        match event {
            StreamEvent::TextDelta { index, delta } => {
                callbacks.on_text_delta(*index, delta).await?;
            }
            StreamEvent::TextDone { index, text } => {
                callbacks.on_text_done(*index, text).await?;
            }
            StreamEvent::ThinkingDelta { index, delta } => {
                callbacks.on_thinking_delta(*index, delta).await?;
            }
            StreamEvent::ThinkingDone { index, content, .. } => {
                callbacks.on_thinking_done(*index, content).await?;
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                callbacks.on_tool_call_start(*index, id, name).await?;
            }
            StreamEvent::ToolCallDone { index, tool_call } => {
                callbacks.on_tool_call_done(*index, tool_call).await?;
            }
            StreamEvent::ResponseDone { finish_reason, .. } => {
                callbacks.on_finish(*finish_reason).await?;
            }
            StreamEvent::Error(e) => {
                let err = HyperError::StreamError(e.message.clone());
                callbacks.on_error(&err).await?;
            }
            // Other events don't trigger callbacks
            _ => {}
        }
        Ok(())
    }
}

/// A simple callback that prints text deltas to stdout.
pub struct PrintCallbacks;

#[async_trait]
impl StreamCallbacks for PrintCallbacks {
    async fn on_text_delta(&mut self, _index: i64, delta: &str) -> Result<(), HyperError> {
        print!("{delta}");
        Ok(())
    }

    async fn on_finish(&mut self, _reason: FinishReason) -> Result<(), HyperError> {
        println!();
        Ok(())
    }
}

/// A callback that collects all text into a String.
pub struct CollectTextCallbacks {
    /// The collected text.
    pub text: String,
}

impl CollectTextCallbacks {
    /// Create a new collector.
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }
}

impl Default for CollectTextCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StreamCallbacks for CollectTextCallbacks {
    async fn on_text_delta(&mut self, _index: i64, delta: &str) -> Result<(), HyperError> {
        self.text.push_str(delta);
        Ok(())
    }
}

#[cfg(test)]
#[path = "callbacks.test.rs"]
mod tests;
