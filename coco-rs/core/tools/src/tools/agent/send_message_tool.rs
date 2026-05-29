//! `SendMessageTool` — deliver a message to a teammate or broadcast.
//!
//! TS: `tools/SendMessageTool/`.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Typed input for [`SendMessageTool`].
///
/// `message` stays `Value` because the wire shape is a union of
/// `string` and structured object variants (`shutdown_request`,
/// `shutdown_response`, `plan_approval_response`). Typing this as
/// `#[serde(untagged)] enum` would be more precise but the runtime
/// branches purely on `message.is_string()` anyway.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SendMessageInput {
    /// Target agent name, "*" for broadcast, or agent ID
    pub to: String,
    /// Brief summary of the message (5-10 words). Required when
    /// `message` is a plain string (used by the leader UI message
    /// stack); structured messages skip it.
    #[serde(default)]
    pub summary: Option<String>,
    /// Message content (string or structured object — see TS
    /// `SendMessageTool.ts` for the structured variants like
    /// `shutdown_request`, `plan_approval_response`).
    pub message: Value,
}

pub struct SendMessageTool;

#[async_trait::async_trait]
impl Tool for SendMessageTool {
    type Input = SendMessageInput;
    coco_tool_runtime::impl_runtime_schema!(SendMessageInput);
    /// Output is `Value` because the wire shape is a tagged union:
    /// bare confirmation string from `agent.send_message` for the
    /// running-agent path, or `{auto_resumed, original_agent_id,
    /// resumed_as, message}` envelope for the terminal-target
    /// auto-resume path.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::SendMessage)
    }
    fn name(&self) -> &str {
        ToolName::SendMessage.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _input: &SendMessageInput, _options: &DescriptionOptions) -> String {
        "Send a message to another agent in the team. Use the agent's name \
         as target, or \"*\" to broadcast to all teammates."
            .into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("send a message to a teammate agent or broadcast")
    }

    /// Render either the prebuilt `message` field (auto-resumed path)
    /// or the bare confirmation string returned by `agent.send_message`.
    fn render_for_model(&self, out: &Value) -> Vec<ToolResultContentPart> {
        let text = if let Some(s) = out.as_str() {
            s.to_string()
        } else if let Some(msg) = out.get("message").and_then(Value::as_str) {
            msg.to_string()
        } else {
            serde_json::to_string(out).unwrap_or_default()
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: SendMessageInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        if input.to.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "target agent name or ID ('to') is required".into(),
                error_code: None,
            });
        }

        // TS `SendMessageTool.ts:668-674`: `summary` is required for plain
        // string messages (used by the leader's UI message-stack); structured
        // messages skip it because the type discriminator carries the intent.
        let is_string_message = input.message.is_string();
        let content = if let Some(s) = input.message.as_str() {
            s.to_string()
        } else {
            serde_json::to_string(&input.message).unwrap_or_default()
        };

        if content.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "message content must be non-empty".into(),
                error_code: None,
            });
        }

        if is_string_message {
            let summary = input.summary.as_deref().unwrap_or("");
            if summary.is_empty() {
                return Err(ToolError::InvalidInput {
                    message: "summary is required when sending a plain-text message \
                              (5-10 word description used by the leader UI)"
                        .into(),
                    error_code: None,
                });
            }
        }

        let to = input.to.as_str();

        // Probe the task handle for the target's current state — drives
        // both the auto-resume branch (terminal target) and the
        // pending-message-queue branch (running target).
        let task_status = if is_string_message {
            match ctx.task_handle.as_ref() {
                Some(h) => h.get_task_status(to).await.ok(),
                None => None,
            }
        } else {
            None
        };

        // TS `SendMessageTool.ts:823-844`: when the target is a known
        // background task in a terminal state (Completed / Failed /
        // Killed), auto-resume instead of routing through the team
        // mailbox. The model thinks it's just sending a message; the
        // resume is transparent.
        //
        // TS does NOT touch `pendingMessages` on this path — it passes
        // the new prompt verbatim to `resumeAgentBackground`. Any prior
        // pending messages stay on the (resumed) task and surface via
        // the `agent_pending_messages` reminder on the next turn
        // (TS `attachments.ts:1085-1101`). Mirror that here: no drain,
        // no prompt-prepend.
        if let Some(info) = task_status.as_ref().filter(|i| i.status.is_terminal()) {
            // Resume needs the parent session id to find the persisted
            // transcript on disk. An empty session id makes the lookup
            // path malformed and surfaces a confusing inner error; reject
            // upfront with a clear message instead.
            let Some(session_id) = ctx
                .session_id_for_history
                .as_deref()
                .filter(|s| !s.is_empty())
            else {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "Agent '{to}' is stopped ({status:?}) and a resume was requested, but \
                         the parent session id is unavailable. Resume is only supported in \
                         persisted sessions — start the session via `coco` (not the \
                         minimal SDK embedding) to enable transcript-backed resume.",
                        status = info.status,
                    ),
                    source: None,
                });
            };
            let resume = ctx
                .agent
                .resume_agent(to, &content, session_id)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!(
                        "Agent '{to}' is stopped ({status:?}) and could not be resumed: {e}",
                        status = info.status,
                    ),
                    source: None,
                })?;
            let new_id = resume.agent_id.as_deref().unwrap_or(to);
            return Ok(ToolResult {
                data: serde_json::json!({
                    "auto_resumed": true,
                    "original_agent_id": to,
                    "resumed_as": new_id,
                    "message": format!(
                        "Agent '{to}' was stopped ({status:?}); resumed it in the background \
                         with your message. New task id: {new_id}. You'll be notified when it finishes.",
                        status = info.status,
                    ),
                }),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
            });
        }

        // TS `SendMessageTool.ts` running-agent path: queue the message
        // onto the recipient's per-task `pendingMessages` FIFO so the
        // recipient's next turn sees it as an `agent_pending_messages`
        // system-reminder (TS `attachments.ts:1085-1101`
        // `getAgentPendingMessageAttachments` drains and maps to
        // `queued_command` attachments). Routing falls through to the
        // mailbox handle as well so multi-process teammates still see
        // the message via their inbox.
        if let Some(info) = task_status.as_ref()
            && info.status == coco_types::TaskStatus::Running
            && info.task_type() == coco_types::TaskType::BgAgent
        {
            let sender = ctx
                .agent_id
                .as_ref()
                .map(|a| a.as_str().to_string())
                .unwrap_or_else(|| "main".into());
            ctx.pending_messages
                .push(
                    to,
                    coco_tool_runtime::PendingMessage {
                        from: sender,
                        text: content.clone(),
                    },
                )
                .await;
        }

        let result =
            ctx.agent
                .send_message(to, &content)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}
