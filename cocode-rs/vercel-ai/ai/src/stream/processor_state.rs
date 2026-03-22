//! Internal accumulation logic for StreamProcessor.

use std::collections::HashMap;

use vercel_ai_provider::language_model::v4::stream::LanguageModelV4StreamPart;

use super::snapshot::FileSnapshot;
use super::snapshot::ReasoningSnapshot;
use super::snapshot::SourceSnapshot;
use super::snapshot::StreamSnapshot;
use super::snapshot::ToolCallSnapshot;

/// Internal state that tracks active segments and accumulates into a snapshot.
pub(super) struct ProcessorState {
    pub snapshot: StreamSnapshot,
    /// Currently active text segment ID.
    active_text_id: Option<String>,
    /// Currently active reasoning segment ID.
    active_reasoning_id: Option<String>,
    /// Map from tool call ID to index in `snapshot.tool_calls`.
    active_tool_inputs: HashMap<String, usize>,
}

impl ProcessorState {
    pub fn new() -> Self {
        Self {
            snapshot: StreamSnapshot::default(),
            active_text_id: None,
            active_reasoning_id: None,
            active_tool_inputs: HashMap::new(),
        }
    }

    /// Update state from a stream part. Returns `true` if a content segment
    /// completed (text end, reasoning end, tool call, or finish).
    pub fn update(&mut self, part: &LanguageModelV4StreamPart) -> bool {
        match part {
            // --- Text ---
            LanguageModelV4StreamPart::TextStart { id, .. } => {
                self.active_text_id = Some(id.clone());
                false
            }
            LanguageModelV4StreamPart::TextDelta { delta, .. } => {
                self.snapshot.text.push_str(delta);
                false
            }
            LanguageModelV4StreamPart::TextEnd { .. } => {
                self.active_text_id = None;
                true
            }

            // --- Reasoning ---
            LanguageModelV4StreamPart::ReasoningStart { id, .. } => {
                self.snapshot.reasoning = Some(ReasoningSnapshot {
                    id: id.clone(),
                    content: String::new(),
                    is_complete: false,
                    signature: None,
                });
                self.active_reasoning_id = Some(id.clone());
                false
            }
            LanguageModelV4StreamPart::ReasoningDelta { delta, .. } => {
                if let Some(r) = &mut self.snapshot.reasoning {
                    r.content.push_str(delta);
                }
                false
            }
            LanguageModelV4StreamPart::ReasoningEnd { .. } => {
                if let Some(r) = &mut self.snapshot.reasoning {
                    r.is_complete = true;
                }
                self.active_reasoning_id = None;
                true
            }

            // --- Tool Input ---
            LanguageModelV4StreamPart::ToolInputStart { id, tool_name, .. } => {
                let idx = self.snapshot.tool_calls.len();
                self.snapshot.tool_calls.push(ToolCallSnapshot {
                    id: id.clone(),
                    tool_name: tool_name.clone(),
                    input_json: String::new(),
                    is_input_complete: false,
                    is_complete: false,
                });
                self.active_tool_inputs.insert(id.clone(), idx);
                false
            }
            LanguageModelV4StreamPart::ToolInputDelta { id, delta, .. } => {
                if let Some(&idx) = self.active_tool_inputs.get(id) {
                    self.snapshot.tool_calls[idx].input_json.push_str(delta);
                }
                false
            }
            LanguageModelV4StreamPart::ToolInputEnd { id, .. } => {
                if let Some(&idx) = self.active_tool_inputs.get(id) {
                    self.snapshot.tool_calls[idx].is_input_complete = true;
                }
                false
            }

            // --- Tool Call (complete) ---
            LanguageModelV4StreamPart::ToolCall(tc) => {
                let input_str = tc.input.to_string();
                if let Some(&idx) = self.active_tool_inputs.get(&tc.tool_call_id) {
                    self.snapshot.tool_calls[idx].is_complete = true;
                    // Use the final input from the ToolCall event
                    self.snapshot.tool_calls[idx].input_json = input_str;
                    self.active_tool_inputs.remove(&tc.tool_call_id);
                } else {
                    // ToolCall without prior ToolInputStart — add directly
                    self.snapshot.tool_calls.push(ToolCallSnapshot {
                        id: tc.tool_call_id.clone(),
                        tool_name: tc.tool_name.clone(),
                        input_json: input_str,
                        is_input_complete: true,
                        is_complete: true,
                    });
                }
                true
            }

            // --- File ---
            LanguageModelV4StreamPart::File(file) => {
                self.snapshot.files.push(FileSnapshot {
                    data: file.data.clone(),
                    media_type: file.media_type.clone(),
                });
                true
            }

            // --- Source ---
            LanguageModelV4StreamPart::Source(source) => {
                self.snapshot.sources.push(SourceSnapshot {
                    id: source.id.clone(),
                    url: source.url.clone().unwrap_or_default(),
                    title: source.title.clone(),
                });
                false
            }

            // --- Stream lifecycle ---
            LanguageModelV4StreamPart::StreamStart { warnings } => {
                self.snapshot.warnings = warnings.clone();
                false
            }

            LanguageModelV4StreamPart::Finish {
                usage,
                finish_reason,
                ..
            } => {
                self.snapshot.usage = Some(usage.clone());
                self.snapshot.finish_reason = Some(finish_reason.clone());
                self.snapshot.is_complete = true;
                true
            }

            // P27: Store response metadata (model ID, request ID, headers) for diagnostics.
            LanguageModelV4StreamPart::ResponseMetadata(meta) => {
                self.snapshot.response_metadata = Some(meta.clone());
                false
            }

            // Events we track but don't complete on
            LanguageModelV4StreamPart::ToolResult(_)
            | LanguageModelV4StreamPart::ToolApprovalRequest(_)
            | LanguageModelV4StreamPart::ReasoningFile(_)
            | LanguageModelV4StreamPart::Raw { .. }
            | LanguageModelV4StreamPart::Error { .. } => false,
        }
    }
}
