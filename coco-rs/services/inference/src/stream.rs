//! Streaming query support — processes LanguageModelV4StreamPart into StreamEvent.

use coco_types::TokenUsage;
use futures::StreamExt;
use std::pin::Pin;
use tokio::sync::mpsc;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::Usage;

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

/// Build a synthetic `LanguageModelV4StreamResult` from a fully-materialized
/// content list.
///
/// Intended for **tests**: mocks that only implement `do_generate` can delegate
/// their `do_stream` to this helper so the streaming agent loop sees the same
/// logical response. Each content block is emitted as a single-chunk
/// start/delta/end trio, followed by a `Finish` event carrying `usage` and
/// `finish_reason`. Non-stream variants (`File`, `Source`, etc.) are ignored.
pub fn synthetic_stream_from_content(
    content: Vec<AssistantContentPart>,
    usage: Usage,
    finish_reason: FinishReason,
) -> LanguageModelV4StreamResult {
    use LanguageModelV4StreamPart as Part;

    let mut parts: Vec<Result<Part, AISdkError>> = Vec::new();
    let mut seg = 0usize;
    for block in content {
        match block {
            AssistantContentPart::Reasoning(r) => {
                let id = format!("reasoning-{seg}");
                seg += 1;
                parts.push(Ok(Part::ReasoningStart {
                    id: id.clone(),
                    provider_metadata: None,
                }));
                parts.push(Ok(Part::ReasoningDelta {
                    id: id.clone(),
                    delta: r.text,
                    provider_metadata: None,
                }));
                parts.push(Ok(Part::ReasoningEnd {
                    id,
                    provider_metadata: None,
                }));
            }
            AssistantContentPart::Text(t) => {
                let id = format!("text-{seg}");
                seg += 1;
                parts.push(Ok(Part::TextStart {
                    id: id.clone(),
                    provider_metadata: None,
                }));
                parts.push(Ok(Part::TextDelta {
                    id: id.clone(),
                    delta: t.text,
                    provider_metadata: None,
                }));
                parts.push(Ok(Part::TextEnd {
                    id,
                    provider_metadata: None,
                }));
            }
            AssistantContentPart::ToolCall(tc) => {
                parts.push(Ok(Part::ToolInputStart {
                    id: tc.tool_call_id.clone(),
                    tool_name: tc.tool_name.clone(),
                    provider_executed: tc.provider_executed,
                    dynamic: None,
                    title: None,
                    provider_metadata: None,
                }));
                let json = serde_json::to_string(&tc.input).unwrap_or_else(|_| "{}".into());
                parts.push(Ok(Part::ToolInputDelta {
                    id: tc.tool_call_id.clone(),
                    delta: json,
                    provider_metadata: None,
                }));
                parts.push(Ok(Part::ToolInputEnd {
                    id: tc.tool_call_id,
                    provider_metadata: None,
                }));
            }
            _ => {}
        }
    }
    parts.push(Ok(Part::Finish {
        usage,
        finish_reason,
        provider_metadata: None,
    }));

    let stream = futures::stream::iter(parts);
    LanguageModelV4StreamResult::new(Box::pin(stream))
}

#[cfg(test)]
#[path = "stream.test.rs"]
mod tests;
