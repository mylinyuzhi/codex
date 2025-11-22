//! Model client bridge for subagent execution.
//!
//! This module provides an abstraction for LLM calls that can be:
//! - Implemented by the real ModelClient for production use
//! - Mocked for testing

use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Trait for executing model calls in subagent context.
///
/// This abstraction allows subagents to make LLM calls without directly
/// depending on ModelClient, enabling:
/// - Easier testing with mock implementations
/// - Different model configurations for different agents
/// - Potential future support for non-OpenAI models
#[async_trait]
pub trait SubagentModelBridge: Send + Sync + std::fmt::Debug {
    /// Execute a model turn and return a receiver for streaming events.
    async fn execute_turn(&self, prompt: Prompt) -> Result<TurnEventReceiver>;

    /// Get the model name being used.
    fn model_name(&self) -> &str;
}

/// Receiver for turn execution events.
pub struct TurnEventReceiver {
    /// Channel receiver for response events.
    pub rx: mpsc::Receiver<Result<ResponseEvent>>,
}

impl TurnEventReceiver {
    /// Create a new receiver from an mpsc channel.
    pub fn new(rx: mpsc::Receiver<Result<ResponseEvent>>) -> Self {
        Self { rx }
    }

    /// Receive the next event.
    pub async fn recv(&mut self) -> Option<Result<ResponseEvent>> {
        self.rx.recv().await
    }
}

/// Type alias for shared model bridge.
pub type SharedModelBridge = Arc<dyn SubagentModelBridge>;

/// Stub implementation for testing that returns immediate completion.
#[derive(Debug)]
pub struct StubModelBridge {
    model: String,
}

impl StubModelBridge {
    /// Create a new stub bridge.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

#[async_trait]
impl SubagentModelBridge for StubModelBridge {
    async fn execute_turn(&self, _prompt: Prompt) -> Result<TurnEventReceiver> {
        let (tx, rx) = mpsc::channel(1);

        // Send a completion event with stub content
        let _ = tx
            .send(Ok(ResponseEvent::Completed {
                response_id: "stub-response".to_string(),
                token_usage: None,
            }))
            .await;

        Ok(TurnEventReceiver::new(rx))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_bridge() {
        let bridge = StubModelBridge::new("test-model");
        assert_eq!(bridge.model_name(), "test-model");

        let prompt = Prompt::default();
        let mut receiver = bridge.execute_turn(prompt).await.unwrap();

        let event = receiver.recv().await;
        assert!(event.is_some());
        if let Some(Ok(ResponseEvent::Completed { response_id, .. })) = event {
            assert_eq!(response_id, "stub-response");
        } else {
            panic!("Expected Completed event");
        }
    }
}
