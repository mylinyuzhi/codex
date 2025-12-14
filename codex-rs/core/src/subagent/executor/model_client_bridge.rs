//! Bridge wrapper for ModelClient to implement SubagentModelBridge trait.
//!
//! This keeps ModelClient unchanged while providing the abstraction needed for subagents.

use super::model_bridge::SubagentModelBridge;
use super::model_bridge::TurnEventReceiver;
use crate::client::ModelClient;
use crate::client_common::Prompt;
use crate::error::Result;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;

/// Bridge wrapper for ModelClient to implement SubagentModelBridge trait.
#[derive(Debug)]
pub struct ModelClientBridge {
    client: ModelClient,
    /// Actual model name for logging/debugging.
    model_name: String,
}

impl ModelClientBridge {
    /// Create a new ModelClientBridge wrapping a ModelClient.
    ///
    /// # Arguments
    /// * `client` - The ModelClient to wrap
    /// * `model_name` - The actual model name for logging/debugging
    pub fn new(client: ModelClient, model_name: String) -> Self {
        Self { client, model_name }
    }
}

#[async_trait]
impl SubagentModelBridge for ModelClientBridge {
    async fn execute_turn(&self, prompt: Prompt) -> Result<TurnEventReceiver> {
        // Get the stream from ModelClient
        let mut stream = self.client.stream(&prompt).await?;

        // Create a channel to forward events
        let (tx, rx) = mpsc::channel(100);

        // Spawn a task to forward events from the stream to the channel
        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                if tx.send(event).await.is_err() {
                    // Receiver dropped, stop forwarding
                    break;
                }
            }
        });

        Ok(TurnEventReceiver::new(rx))
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    // Integration tests would require a real ModelClient,
    // which needs API credentials. Unit tests for the bridge
    // are limited without mocking the ModelClient.
}
