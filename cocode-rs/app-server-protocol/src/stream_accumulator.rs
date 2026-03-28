//! Slim stateful accumulator for [`StreamEvent`] → [`ServerNotification`].
//!
//! Handles the 7 streaming events that need stateful accumulation. Protocol
//! events flow directly as `CoreEvent::Protocol(ServerNotification)`.

use std::collections::HashMap;

use cocode_protocol::ToolName;
use cocode_protocol::ToolResultContent;
use cocode_protocol::server_notification::*;
use cocode_protocol::stream_event::StreamEvent;

/// Stateful accumulator that converts raw streaming deltas into
/// protocol-level `ServerNotification`s.
///
/// Maintains state for:
/// - Text buffer accumulation (TextDelta → AgentMessage items)
/// - Thinking buffer accumulation (ThinkingDelta → Reasoning items)
/// - Active tool call tracking (ToolUseQueued/Started/Completed → Item lifecycle)
pub struct StreamAccumulator {
    turn_id: String,
    /// Active tool-call items keyed by call_id.
    active_items: HashMap<String, ThreadItem>,
    /// Accumulated agent message text for the current turn.
    text_buffer: String,
    /// Assigned item ID for the current text message.
    text_item_id: Option<String>,
    /// Accumulated reasoning text for the current turn.
    thinking_buffer: String,
    /// Assigned item ID for the current thinking block.
    thinking_item_id: Option<String>,
    /// Counter for generating item IDs.
    item_counter: i32,
}

impl StreamAccumulator {
    /// Create a new accumulator for the given turn.
    pub fn new(turn_id: String) -> Self {
        Self {
            turn_id,
            active_items: HashMap::new(),
            text_buffer: String::new(),
            text_item_id: None,
            thinking_buffer: String::new(),
            thinking_item_id: None,
            item_counter: 0,
        }
    }

    fn next_item_id(&mut self) -> String {
        self.item_counter += 1;
        format!("item_{}", self.item_counter)
    }

    /// Flush accumulated text/reasoning buffers as completed items.
    pub fn flush(&mut self) -> Vec<ServerNotification> {
        let mut notifications = Vec::new();

        if let Some(ref thinking_id) = self.thinking_item_id {
            if !self.thinking_buffer.is_empty() {
                notifications.push(ServerNotification::ItemCompleted(ItemEventParams {
                    item: ThreadItem {
                        id: thinking_id.clone(),
                        details: ThreadItemDetails::Reasoning(ReasoningItem {
                            text: std::mem::take(&mut self.thinking_buffer),
                        }),
                    },
                }));
            }
            self.thinking_item_id = None;
        }

        if let Some(ref text_id) = self.text_item_id {
            if !self.text_buffer.is_empty() {
                notifications.push(ServerNotification::ItemCompleted(ItemEventParams {
                    item: ThreadItem {
                        id: text_id.clone(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: std::mem::take(&mut self.text_buffer),
                        }),
                    },
                }));
            }
            self.text_item_id = None;
        }

        notifications
    }

    /// Snapshot the accumulated text buffer (before flush drains it).
    pub fn accumulated_text(&self) -> &str {
        &self.text_buffer
    }

    /// Process a streaming event into zero or more protocol notifications.
    pub fn process(&mut self, event: StreamEvent) -> Vec<ServerNotification> {
        match event {
            StreamEvent::TextDelta { delta, .. } => self.handle_text_delta(delta),
            StreamEvent::ThinkingDelta { delta, .. } => self.handle_thinking_delta(delta),
            StreamEvent::ToolUseQueued {
                call_id,
                name,
                input,
            } => self.handle_tool_queued(call_id, name, input),
            StreamEvent::ToolUseStarted { call_id, .. } => self.handle_tool_started(call_id),
            StreamEvent::ToolUseCompleted {
                call_id,
                output,
                is_error,
            } => self.handle_tool_completed(call_id, output, is_error),
            StreamEvent::McpToolCallBegin {
                server,
                tool,
                call_id,
            } => self.handle_mcp_begin(server, tool, call_id),
            StreamEvent::McpToolCallEnd {
                call_id, is_error, ..
            } => self.handle_mcp_end(call_id, is_error),
        }
    }

    fn handle_text_delta(&mut self, delta: String) -> Vec<ServerNotification> {
        let mut out = Vec::new();

        // Close reasoning item when text starts
        if let Some(thinking_id) = self.thinking_item_id.take()
            && !self.thinking_buffer.is_empty()
        {
            out.push(ServerNotification::ItemCompleted(ItemEventParams {
                item: ThreadItem {
                    id: thinking_id,
                    details: ThreadItemDetails::Reasoning(ReasoningItem {
                        text: std::mem::take(&mut self.thinking_buffer),
                    }),
                },
            }));
        }

        if self.text_item_id.is_none() {
            let id = self.next_item_id();
            out.push(ServerNotification::ItemStarted(ItemEventParams {
                item: ThreadItem {
                    id: id.clone(),
                    details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                        text: String::new(),
                    }),
                },
            }));
            self.text_item_id = Some(id);
        }

        let item_id = self.text_item_id.clone().unwrap_or_default();
        self.text_buffer.push_str(&delta);
        out.push(ServerNotification::AgentMessageDelta(
            AgentMessageDeltaParams {
                item_id,
                turn_id: self.turn_id.clone(),
                delta,
            },
        ));
        out
    }

    fn handle_thinking_delta(&mut self, delta: String) -> Vec<ServerNotification> {
        let mut out = Vec::new();

        if self.thinking_item_id.is_none() {
            let id = self.next_item_id();
            out.push(ServerNotification::ItemStarted(ItemEventParams {
                item: ThreadItem {
                    id: id.clone(),
                    details: ThreadItemDetails::Reasoning(ReasoningItem {
                        text: String::new(),
                    }),
                },
            }));
            self.thinking_item_id = Some(id);
        }

        let item_id = self.thinking_item_id.clone().unwrap_or_default();
        self.thinking_buffer.push_str(&delta);
        out.push(ServerNotification::ReasoningDelta(ReasoningDeltaParams {
            item_id,
            turn_id: self.turn_id.clone(),
            delta,
        }));
        out
    }

    fn handle_tool_queued(
        &mut self,
        call_id: String,
        name: String,
        input: serde_json::Value,
    ) -> Vec<ServerNotification> {
        let item_id = self.next_item_id();
        let item = build_tool_item(&item_id, &name, &input, ItemStatus::InProgress);
        self.active_items.insert(call_id, item.clone());
        vec![ServerNotification::ItemStarted(ItemEventParams { item })]
    }

    fn handle_tool_started(&self, call_id: String) -> Vec<ServerNotification> {
        if let Some(item) = self.active_items.get(&call_id) {
            vec![ServerNotification::ItemUpdated(ItemEventParams {
                item: item.clone(),
            })]
        } else {
            vec![]
        }
    }

    fn handle_tool_completed(
        &mut self,
        call_id: String,
        output: ToolResultContent,
        is_error: bool,
    ) -> Vec<ServerNotification> {
        if let Some(mut item) = self.active_items.remove(&call_id) {
            let status = if is_error {
                ItemStatus::Failed
            } else {
                ItemStatus::Completed
            };
            update_item_status(&mut item, status, &output);
            vec![ServerNotification::ItemCompleted(ItemEventParams { item })]
        } else {
            vec![]
        }
    }

    fn handle_mcp_begin(
        &mut self,
        server: String,
        tool: String,
        call_id: String,
    ) -> Vec<ServerNotification> {
        let item = ThreadItem {
            id: call_id.clone(),
            details: ThreadItemDetails::McpToolCall(McpToolCallItem {
                server,
                tool,
                arguments: serde_json::Value::Null,
                result: None,
                error: None,
                status: ItemStatus::InProgress,
            }),
        };
        self.active_items.insert(call_id, item.clone());
        vec![ServerNotification::ItemStarted(ItemEventParams { item })]
    }

    fn handle_mcp_end(&mut self, call_id: String, is_error: bool) -> Vec<ServerNotification> {
        if let Some(mut item) = self.active_items.remove(&call_id) {
            let status = if is_error {
                ItemStatus::Failed
            } else {
                ItemStatus::Completed
            };
            if let ThreadItemDetails::McpToolCall(ref mut mcp) = item.details {
                mcp.status = status;
            }
            vec![ServerNotification::ItemCompleted(ItemEventParams { item })]
        } else {
            vec![]
        }
    }
}

/// Build a `ThreadItem` for a tool call based on the tool name.
fn build_tool_item(
    item_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    status: ItemStatus,
) -> ThreadItem {
    let details = match tool_name {
        name if name == ToolName::Bash.as_str() => {
            let command = input["command"].as_str().unwrap_or("").to_string();
            ThreadItemDetails::CommandExecution(CommandExecutionItem {
                command,
                aggregated_output: String::new(),
                exit_code: None,
                status,
            })
        }
        name if name == ToolName::Edit.as_str()
            || name == ToolName::Write.as_str()
            || name == ToolName::NotebookEdit.as_str() =>
        {
            let path = input["file_path"]
                .as_str()
                .or_else(|| input["path"].as_str())
                .unwrap_or("")
                .to_string();
            let kind = if tool_name == ToolName::Write.as_str() {
                FileChangeKind::Add
            } else {
                FileChangeKind::Update
            };
            ThreadItemDetails::FileChange(FileChangeItem {
                changes: vec![FileChange { path, kind }],
                status,
            })
        }
        name if name == ToolName::WebSearch.as_str() || name == ToolName::WebFetch.as_str() => {
            let query = input["query"]
                .as_str()
                .or_else(|| input["url"].as_str())
                .unwrap_or("")
                .to_string();
            ThreadItemDetails::WebSearch(WebSearchItem { query, status })
        }
        name if name.starts_with("mcp__") => {
            let parts: Vec<&str> = name.splitn(3, "__").collect();
            let (server, tool) = if parts.len() >= 3 {
                (parts[1].to_string(), parts[2].to_string())
            } else {
                (String::new(), name.to_string())
            };
            ThreadItemDetails::McpToolCall(McpToolCallItem {
                server,
                tool,
                arguments: input.clone(),
                result: None,
                error: None,
                status,
            })
        }
        name if name == ToolName::Task.as_str() || name == "Agent" => {
            let agent_type = input["subagent_type"]
                .as_str()
                .unwrap_or("general-purpose")
                .to_string();
            let description = input["description"].as_str().unwrap_or("").to_string();
            ThreadItemDetails::Subagent(SubagentItem {
                agent_id: item_id.to_string(),
                agent_type,
                description,
                is_background: input["run_in_background"].as_bool().unwrap_or(false),
                result: None,
                status,
            })
        }
        _ => ThreadItemDetails::ToolCall(GenericToolCallItem {
            tool: tool_name.to_string(),
            input: input.clone(),
            output: None,
            is_error: false,
            status,
        }),
    };

    ThreadItem {
        id: item_id.to_string(),
        details,
    }
}

/// Update the status and output of a thread item on completion.
fn update_item_status(item: &mut ThreadItem, status: ItemStatus, output: &ToolResultContent) {
    let output_str = match output {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Structured(v) => v.to_string(),
    };

    match &mut item.details {
        ThreadItemDetails::CommandExecution(cmd) => {
            cmd.status = status;
            cmd.aggregated_output = output_str;
            if status == ItemStatus::Completed {
                cmd.exit_code = Some(0);
            }
        }
        ThreadItemDetails::FileChange(fc) => {
            fc.status = status;
        }
        ThreadItemDetails::McpToolCall(mcp) => {
            mcp.status = status;
            if status == ItemStatus::Failed {
                mcp.error = Some(McpToolCallError {
                    message: output_str,
                });
            } else {
                mcp.result = Some(McpToolCallResult {
                    content: vec![serde_json::Value::String(output_str)],
                    structured_content: None,
                });
            }
        }
        ThreadItemDetails::WebSearch(ws) => {
            ws.status = status;
        }
        ThreadItemDetails::Subagent(sub) => {
            sub.status = status;
            sub.result = Some(output_str);
        }
        ThreadItemDetails::ToolCall(tc) => {
            tc.status = status;
            tc.output = Some(output_str);
            tc.is_error = status == ItemStatus::Failed;
        }
        ThreadItemDetails::AgentMessage(_)
        | ThreadItemDetails::Reasoning(_)
        | ThreadItemDetails::Error(_) => {}
    }
}
