//! Streaming query support — processes LanguageModelV4StreamPart into StreamEvent.

use coco_types::TokenUsage;
use futures::StreamExt;
use std::pin::Pin;
use tokio::sync::mpsc;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::LanguageModelV4StreamPart;

/// Events emitted during streaming inference.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta (incremental text output).
    TextDelta { text: String },
    /// Reasoning/thinking delta.
    ReasoningDelta { text: String },
    /// Tool call started.
    ToolCallStart { id: String, tool_name: String },
    /// Tool call input delta (JSON fragment).
    ToolCallDelta { id: String, delta: String },
    /// Tool call input complete.
    ToolCallEnd { id: String },
    /// Stream finished — final usage.
    Finish {
        usage: TokenUsage,
        stop_reason: String,
    },
    /// Error during streaming.
    Error { message: String },
}

/// Process a vercel-ai stream into StreamEvents sent via channel.
pub async fn process_stream(
    mut stream: Pin<
        Box<dyn futures::Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>,
    >,
    tx: mpsc::Sender<StreamEvent>,
) {
    while let Some(part) = stream.next().await {
        let event = match part {
            Ok(LanguageModelV4StreamPart::TextDelta { delta, .. }) => {
                StreamEvent::TextDelta { text: delta }
            }
            Ok(LanguageModelV4StreamPart::ReasoningDelta { delta, .. }) => {
                StreamEvent::ReasoningDelta { text: delta }
            }
            Ok(LanguageModelV4StreamPart::ToolInputStart { id, tool_name, .. }) => {
                StreamEvent::ToolCallStart { id, tool_name }
            }
            Ok(LanguageModelV4StreamPart::ToolInputDelta { id, delta, .. }) => {
                StreamEvent::ToolCallDelta { id, delta }
            }
            Ok(LanguageModelV4StreamPart::ToolInputEnd { id, .. }) => {
                StreamEvent::ToolCallEnd { id }
            }
            Ok(LanguageModelV4StreamPart::Finish {
                usage,
                finish_reason,
                ..
            }) => StreamEvent::Finish {
                usage: TokenUsage {
                    input_tokens: usage.input_tokens.total.unwrap_or(0) as i64,
                    output_tokens: usage.output_tokens.total.unwrap_or(0) as i64,
                    cache_read_input_tokens: usage.input_tokens.cache_read.unwrap_or(0) as i64,
                    cache_creation_input_tokens: usage.input_tokens.cache_write.unwrap_or(0) as i64,
                },
                stop_reason: finish_reason.unified.to_string(),
            },
            Ok(LanguageModelV4StreamPart::Error { error }) => StreamEvent::Error {
                message: error.message,
            },
            Ok(_) => continue, // Skip other events (StreamStart, Raw, etc.)
            Err(e) => StreamEvent::Error {
                message: e.to_string(),
            },
        };

        if tx.send(event).await.is_err() {
            break; // Receiver dropped
        }
    }
}
