//! Streaming query support — processes LanguageModelV4StreamPart into StreamEvent.

use coco_types::TokenUsage;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::trace;
use tracing::warn;
use vercel_ai::StreamProcessor;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::Usage;

pub use vercel_ai::StreamMetrics;
pub use vercel_ai::StreamProcessorConfig;

const DEFAULT_PROCESS_STREAM_STALL_THRESHOLD: Duration = Duration::from_secs(30);

/// Events emitted during streaming inference.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta (incremental text output).
    TextDelta { text: String },
    /// Reasoning/thinking delta.
    ReasoningDelta { text: String },
    /// Reasoning/thinking block ended. Carries the provider-supplied
    /// metadata (notably `anthropic.signature` for the Anthropic-shape
    /// API and any compatible providers like DeepSeek's `/anthropic/v1`
    /// endpoint). The agent loop must thread this back into the
    /// `ReasoningPart.provider_metadata` of the assistant message it
    /// records, otherwise the next request will drop the thinking
    /// block and the API will reject it with `content[].thinking must
    /// be passed back`. `None` for providers that don't ship metadata.
    ReasoningEnd {
        provider_metadata: Option<crate::ProviderMetadata>,
    },
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
        metrics: StreamMetrics,
    },
    /// Error during streaming.
    Error {
        message: String,
        metrics: StreamMetrics,
    },
}

/// Process a vercel-ai stream into StreamEvents sent via channel.
pub async fn process_stream(
    stream: Pin<
        Box<dyn futures::Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>,
    >,
    tx: mpsc::Sender<StreamEvent>,
) {
    process_stream_with_config(stream, tx, default_process_stream_config()).await;
}

/// Default stream processor config for the inference event bridge.
///
/// Idle timeout is disabled here to preserve the historical `process_stream`
/// behavior; callers that want watchdog semantics should pass an explicit
/// timeout through [`process_stream_with_config`].
pub fn default_process_stream_config() -> StreamProcessorConfig {
    StreamProcessorConfig::default()
        .without_idle_timeout()
        .with_stall_threshold(DEFAULT_PROCESS_STREAM_STALL_THRESHOLD)
}

/// Process a vercel-ai stream into StreamEvents using explicit processor config.
pub async fn process_stream_with_config(
    stream: Pin<
        Box<dyn futures::Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>,
    >,
    tx: mpsc::Sender<StreamEvent>,
    config: StreamProcessorConfig,
) {
    let mut processor = StreamProcessor::from_stream_with_config(stream, config);
    let mut reported_stall_count = 0;
    let mut emitted_events: i64 = 0;

    while let Some(result) = processor.next().await {
        let (event, metrics) = match result {
            Ok((part, _)) => {
                let metrics = processor.metrics();
                (stream_event_from_part(part, metrics), metrics)
            }
            Err(e) => {
                let metrics = processor.metrics();
                (
                    Some(StreamEvent::Error {
                        message: e.to_string(),
                        metrics,
                    }),
                    metrics,
                )
            }
        };

        if metrics.stall_count > reported_stall_count {
            reported_stall_count = metrics.stall_count;
            warn!(
                stall_count = metrics.stall_count,
                total_stall_ms = metrics.total_stall_ms,
                "streaming stall detected"
            );
        }

        let Some(event) = event else {
            continue;
        };

        // Per-chunk trace — opt-in via `coco_inference::stream=trace`. Names
        // map directly to StreamEvent variants so a tail can grep for
        // `event=text_delta`, `event=tool_call_delta`, etc.
        match &event {
            StreamEvent::TextDelta { text } => {
                trace!(event = "text_delta", chars = text.len(), "stream event")
            }
            StreamEvent::ReasoningDelta { text } => trace!(
                event = "reasoning_delta",
                chars = text.len(),
                "stream event"
            ),
            StreamEvent::ReasoningEnd { .. } => trace!(event = "reasoning_end", "stream event"),
            StreamEvent::ToolCallStart { id, tool_name } => {
                debug!(
                    event = "tool_call_start",
                    id = %id,
                    tool_name = %tool_name,
                    "stream event"
                )
            }
            StreamEvent::ToolCallDelta { id, delta } => trace!(
                event = "tool_call_delta",
                id = %id,
                chars = delta.len(),
                "stream event"
            ),
            StreamEvent::ToolCallEnd { id } => debug!(
                event = "tool_call_end",
                id = %id,
                "stream event"
            ),
            StreamEvent::Finish {
                stop_reason, usage, ..
            } => debug!(
                event = "finish",
                stop_reason = %stop_reason,
                tokens_in = usage.input_tokens,
                tokens_out = usage.output_tokens,
                emitted = emitted_events,
                "stream event"
            ),
            StreamEvent::Error { message, .. } => warn!(
                event = "error",
                message = %message,
                emitted = emitted_events,
                "stream event"
            ),
        }
        emitted_events += 1;

        if tx.send(event).await.is_err() {
            break; // Receiver dropped
        }
    }
}

fn stream_event_from_part(
    part: LanguageModelV4StreamPart,
    metrics: StreamMetrics,
) -> Option<StreamEvent> {
    match part {
        LanguageModelV4StreamPart::TextDelta { delta, .. } => {
            Some(StreamEvent::TextDelta { text: delta })
        }
        LanguageModelV4StreamPart::ReasoningDelta { delta, .. } => {
            Some(StreamEvent::ReasoningDelta { text: delta })
        }
        LanguageModelV4StreamPart::ReasoningEnd {
            provider_metadata, ..
        } => Some(StreamEvent::ReasoningEnd { provider_metadata }),
        LanguageModelV4StreamPart::ToolInputStart { id, tool_name, .. } => {
            Some(StreamEvent::ToolCallStart { id, tool_name })
        }
        LanguageModelV4StreamPart::ToolInputDelta { id, delta, .. } => {
            Some(StreamEvent::ToolCallDelta { id, delta })
        }
        LanguageModelV4StreamPart::ToolInputEnd { id, .. } => Some(StreamEvent::ToolCallEnd { id }),
        LanguageModelV4StreamPart::Finish {
            usage,
            finish_reason,
            ..
        } => Some(StreamEvent::Finish {
            usage: token_usage_from_provider_usage(&usage),
            stop_reason: finish_reason.unified.to_string(),
            metrics,
        }),
        LanguageModelV4StreamPart::Error { error } => Some(StreamEvent::Error {
            message: error.message,
            metrics,
        }),
        _ => None,
    }
}

fn token_usage_from_provider_usage(usage: &Usage) -> TokenUsage {
    let input_total = u64_to_i64(usage.input_tokens.total.unwrap_or(0));
    let output_total = u64_to_i64(usage.output_tokens.total.unwrap_or(0));
    TokenUsage {
        input_tokens: input_total,
        output_tokens: output_total,
        total_tokens: input_total + output_total,
        input_token_details: coco_types::InputTokenDetails {
            no_cache_tokens: u64_to_i64(usage.input_tokens.no_cache.unwrap_or(0)),
            cache_read_tokens: u64_to_i64(usage.input_tokens.cache_read.unwrap_or(0)),
            cache_write_tokens: u64_to_i64(usage.input_tokens.cache_write.unwrap_or(0)),
        },
        // Reasoning + text breakdown when the provider reports them.
        // For DeepSeek V4 / GPT-5 thinking / Claude extended thinking
        // these are non-zero on every reasoning-emitting turn. `0` when
        // the provider's wire shape doesn't separate the two.
        output_token_details: coco_types::OutputTokenDetails {
            text_tokens: u64_to_i64(usage.output_tokens.text.unwrap_or(0)),
            reasoning_tokens: u64_to_i64(usage.output_tokens.reasoning.unwrap_or(0)),
        },
    }
}

fn u64_to_i64(value: u64) -> i64 {
    value.try_into().unwrap_or(i64::MAX)
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
