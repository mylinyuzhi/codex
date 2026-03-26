//! Maps internal `LoopEvent` to client-facing `ServerNotification`.
//!
//! This is the translation layer between cocode-rs internals and the
//! universal protocol. Only externally-relevant events are mapped;
//! UI-only events (PluginDataReady, OutputStylesReady, etc.) are dropped.

use std::collections::HashMap;

use cocode_app_server_protocol::*;
use cocode_protocol::LoopEvent;
use cocode_protocol::ToolName;

/// Stateful mapper that translates `LoopEvent`s into `ServerNotification`s.
///
/// Maintains state for accumulating text/thinking deltas and tracking
/// active items (tool calls in progress).
pub struct EventMapper {
    turn_id: String,
    /// Active tool-call items keyed by call_id.
    active_items: HashMap<String, ThreadItem>,
    /// Accumulated agent message text for the current turn.
    text_buffer: String,
    /// Assigned item ID for the current text message (set on first delta).
    text_item_id: Option<String>,
    /// Accumulated reasoning text for the current turn.
    thinking_buffer: String,
    /// Assigned item ID for the current thinking block (set on first delta).
    thinking_item_id: Option<String>,
    /// Counter for generating item IDs.
    item_counter: i32,
}

impl EventMapper {
    /// Create a new mapper for the given turn.
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
    ///
    /// Call this when a turn completes to emit final `item/completed`
    /// notifications for any accumulated content.
    pub fn flush(&mut self) -> Vec<ServerNotification> {
        let mut notifications = Vec::new();

        // Flush thinking buffer as a completed ReasoningItem
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

        // Flush text buffer as a completed AgentMessage
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

    /// Map a `LoopEvent` to zero or more `ServerNotification`s.
    pub fn map(&mut self, event: LoopEvent) -> Vec<ServerNotification> {
        match event {
            // ── Content streaming ───────────────────────────────────
            LoopEvent::TextDelta { delta, .. } => {
                let mut notifications = Vec::new();

                // Close reasoning item when text starts (thinking → text transition)
                if let Some(thinking_id) = self.thinking_item_id.take()
                    && !self.thinking_buffer.is_empty()
                {
                    notifications.push(ServerNotification::ItemCompleted(ItemEventParams {
                        item: ThreadItem {
                            id: thinking_id,
                            details: ThreadItemDetails::Reasoning(ReasoningItem {
                                text: std::mem::take(&mut self.thinking_buffer),
                            }),
                        },
                    }));
                }

                // Start agent message item on first delta
                if self.text_item_id.is_none() {
                    let id = self.next_item_id();
                    notifications.push(ServerNotification::ItemStarted(ItemEventParams {
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
                notifications.push(ServerNotification::AgentMessageDelta(
                    AgentMessageDeltaParams {
                        item_id,
                        turn_id: self.turn_id.clone(),
                        delta,
                    },
                ));
                notifications
            }

            LoopEvent::ThinkingDelta { delta, .. } => {
                let mut notifications = Vec::new();

                // Start reasoning item on first delta
                if self.thinking_item_id.is_none() {
                    let id = self.next_item_id();
                    notifications.push(ServerNotification::ItemStarted(ItemEventParams {
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
                notifications.push(ServerNotification::ReasoningDelta(ReasoningDeltaParams {
                    item_id,
                    turn_id: self.turn_id.clone(),
                    delta,
                }));
                notifications
            }

            // ── Tool lifecycle ──────────────────────────────────────
            LoopEvent::ToolUseQueued {
                call_id,
                name,
                input,
            } => {
                let item_id = self.next_item_id();
                let item = build_tool_item(&item_id, &name, &input, ItemStatus::InProgress);
                self.active_items.insert(call_id, item.clone());
                vec![ServerNotification::ItemStarted(ItemEventParams { item })]
            }

            LoopEvent::ToolUseStarted { call_id, .. } => {
                if let Some(item) = self.active_items.get(&call_id) {
                    vec![ServerNotification::ItemUpdated(ItemEventParams {
                        item: item.clone(),
                    })]
                } else {
                    vec![]
                }
            }

            LoopEvent::ToolUseCompleted {
                call_id,
                output,
                is_error,
            } => {
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

            // ── Sub-agent events ────────────────────────────────────
            LoopEvent::SubagentSpawned {
                agent_id,
                agent_type,
                description,
                color,
            } => vec![ServerNotification::SubagentSpawned(SubagentSpawnedParams {
                agent_id,
                agent_type,
                description,
                color,
            })],

            LoopEvent::SubagentCompleted { agent_id, result } => {
                vec![ServerNotification::SubagentCompletedParams(
                    SubagentCompletedParams { agent_id, result },
                )]
            }

            LoopEvent::SubagentBackgrounded {
                agent_id,
                output_file,
            } => vec![ServerNotification::SubagentBackgrounded(
                SubagentBackgroundedParams {
                    agent_id,
                    output_file: output_file.to_string_lossy().into_owned(),
                },
            )],

            // ── MCP events ──────────────────────────────────────────
            LoopEvent::McpToolCallBegin {
                server,
                tool,
                call_id,
            } => {
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

            LoopEvent::McpToolCallEnd {
                call_id, is_error, ..
            } => {
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

            LoopEvent::McpStartupUpdate { server, status } => {
                vec![ServerNotification::McpStartupStatus(
                    McpStartupStatusParams {
                        server,
                        status: format!("{status:?}"),
                    },
                )]
            }

            LoopEvent::McpStartupComplete { servers, failed } => {
                vec![ServerNotification::McpStartupComplete(
                    McpStartupCompleteParams {
                        servers: servers
                            .into_iter()
                            .map(|s| McpServerInfoParams {
                                name: s.name,
                                tool_count: s.tool_count,
                            })
                            .collect(),
                        failed: failed
                            .into_iter()
                            .map(|(name, error)| McpServerFailure { name, error })
                            .collect(),
                    },
                )]
            }

            // ── Compaction events ───────────────────────────────────
            LoopEvent::CompactionCompleted {
                removed_messages,
                summary_tokens,
            } => vec![ServerNotification::ContextCompacted(
                ContextCompactedParams {
                    removed_messages,
                    summary_tokens,
                },
            )],

            LoopEvent::ContextUsageWarning {
                estimated_tokens,
                warning_threshold,
                percent_left,
            } => vec![ServerNotification::ContextUsageWarning(
                ContextUsageWarningParams {
                    estimated_tokens,
                    warning_threshold,
                    percent_left,
                },
            )],

            // ── Error events ────────────────────────────────────────
            LoopEvent::Error { error } => {
                vec![ServerNotification::Error(ErrorNotificationParams {
                    message: format!("{error:?}"),
                    category: Some("internal".into()),
                    retryable: false,
                })]
            }

            LoopEvent::ApiError { error, retry_info } => {
                vec![ServerNotification::Error(ErrorNotificationParams {
                    message: error.message,
                    category: Some("api".into()),
                    retryable: retry_info.is_some(),
                })]
            }

            // ── Background task events ────────────────────────────────
            LoopEvent::BackgroundTaskStarted { task_id, task_type } => {
                vec![ServerNotification::TaskStarted(TaskStartedParams {
                    task_id,
                    task_type: format!("{task_type:?}"),
                })]
            }

            LoopEvent::BackgroundTaskCompleted { task_id, result } => {
                vec![ServerNotification::TaskCompleted(TaskCompletedParams {
                    task_id,
                    result,
                    is_error: false,
                })]
            }

            // ── Turn lifecycle (additional) ──────────────────────────
            LoopEvent::Interrupted => {
                vec![ServerNotification::TurnInterrupted(TurnInterruptedParams {
                    turn_id: None,
                })]
            }

            LoopEvent::MaxTurnsReached => {
                vec![ServerNotification::MaxTurnsReached(MaxTurnsReachedParams {
                    max_turns: None,
                })]
            }

            // ── Model events ─────────────────────────────────────────
            LoopEvent::ModelFallbackStarted { from, to, reason } => {
                vec![ServerNotification::ModelFallbackStarted(
                    ModelFallbackStartedParams {
                        from_model: from,
                        to_model: to,
                        reason,
                    },
                )]
            }

            // ── Permission events ────────────────────────────────────
            LoopEvent::PermissionModeChanged { mode } => {
                vec![ServerNotification::PermissionModeChanged(
                    PermissionModeChangedParams {
                        mode: format!("{mode:?}"),
                    },
                )]
            }

            // ── Rate limit events ──────────────────────────────────
            LoopEvent::RateLimit { info } => {
                vec![ServerNotification::RateLimit(RateLimitParams { info })]
            }

            // ── Events intentionally dropped ─────────────────────────
            // UI-only events internal to TUI rendering, plus events
            // handled as ServerRequest in the turn loop (not notifications).
            //
            // QuestionAsked: handled as ServerRequest::RequestUserInput in turn loop
            // ApprovalRequired: handled as ServerRequest::AskForApproval in turn loop
            LoopEvent::QuestionAsked { .. }
            | LoopEvent::StreamRequestStart
            | LoopEvent::StreamRequestEnd { .. }
            | LoopEvent::TurnStarted { .. }
            | LoopEvent::TurnCompleted { .. }
            | LoopEvent::ToolCallDelta { .. }
            | LoopEvent::StreamEvent { .. }
            | LoopEvent::ToolProgress { .. }
            | LoopEvent::ToolExecutionAborted { .. }
            | LoopEvent::ApprovalRequired { .. }
            | LoopEvent::ApprovalResponse { .. }
            | LoopEvent::PermissionChecked { .. }
            | LoopEvent::SubagentProgress { .. }
            | LoopEvent::BackgroundTaskProgress { .. }
            | LoopEvent::AllAgentsKilled { .. }
            | LoopEvent::CompactionStarted
            | LoopEvent::MicroCompactionStarted { .. }
            | LoopEvent::MicroCompactionApplied { .. }
            | LoopEvent::SessionMemoryCompactApplied { .. }
            | LoopEvent::CompactionSkippedByHook { .. }
            | LoopEvent::CompactionRetry { .. }
            | LoopEvent::CompactionFailed { .. }
            | LoopEvent::CompactionCircuitBreakerOpen { .. }
            | LoopEvent::MemoryAttachmentsCleared { .. }
            | LoopEvent::PostCompactHooksExecuted { .. }
            | LoopEvent::CompactBoundaryInserted { .. }
            | LoopEvent::InvokedSkillsRestored { .. }
            | LoopEvent::ContextRestored { .. }
            | LoopEvent::SessionMemoryExtractionStarted { .. }
            | LoopEvent::SessionMemoryExtractionCompleted { .. }
            | LoopEvent::SessionMemoryExtractionFailed { .. }
            | LoopEvent::ModelFallbackCompleted
            | LoopEvent::Tombstone { .. }
            | LoopEvent::Retry { .. }
            | LoopEvent::ElicitationRequested { .. }
            | LoopEvent::PlanModeEntered { .. }
            | LoopEvent::PlanModeExited { .. }
            | LoopEvent::ContextCleared { .. }
            | LoopEvent::HookExecuted { .. }
            | LoopEvent::StreamStallDetected { .. }
            | LoopEvent::StreamWatchdogWarning { .. }
            | LoopEvent::PromptCacheHit { .. }
            | LoopEvent::PromptCacheMiss
            | LoopEvent::SpeculativeStarted { .. }
            | LoopEvent::SpeculativeCommitted { .. }
            | LoopEvent::SpeculativeRolledBack { .. }
            | LoopEvent::CommandQueued { .. }
            | LoopEvent::CommandDequeued { .. }
            | LoopEvent::QueueStateChanged { .. }
            | LoopEvent::PluginAgentsLoaded { .. }
            | LoopEvent::PluginDataReady { .. }
            | LoopEvent::OutputStylesReady { .. }
            | LoopEvent::SystemReminderDisplay { .. }
            | LoopEvent::RewindCompleted { .. }
            | LoopEvent::RewindFailed { .. }
            | LoopEvent::RewindCheckpointsReady { .. }
            | LoopEvent::DiffStatsReady { .. }
            | LoopEvent::SummarizeCompleted { .. }
            | LoopEvent::SummarizeFailed { .. }
            | LoopEvent::CronJobFired { .. }
            | LoopEvent::CronJobDisabled { .. }
            | LoopEvent::CronJobsMissed { .. }
            | LoopEvent::CostWarningThresholdReached { .. }
            | LoopEvent::SandboxApprovalRequired { .. }
            | LoopEvent::SandboxStateChanged { .. }
            | LoopEvent::SandboxViolationsDetected { .. }
            | LoopEvent::FastModeChanged { .. } => vec![],
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
            // MCP tools follow pattern: mcp__<server>__<tool>
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
        // The subagent tool is called "Task" internally but the model prompt
        // may use "Agent" (Claude Code compatibility). Match both.
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
fn update_item_status(
    item: &mut ThreadItem,
    status: ItemStatus,
    output: &cocode_protocol::ToolResultContent,
) {
    let output_str = match output {
        cocode_protocol::ToolResultContent::Text(s) => s.clone(),
        cocode_protocol::ToolResultContent::Structured(v) => v.to_string(),
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
