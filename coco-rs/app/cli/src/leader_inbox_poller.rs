//! Continuous leader-side inbox poller for cross-process teammate
//! messages, idle notifications, and permission requests.
//!
//! TS: `hooks/useInboxPoller.ts` — a 1s `useInterval` poll (plus an initial
//! poll on mount) that scans the team-lead inbox continuously, independent
//! of whether the leader is taking a turn. This mirrors the leader branch
//! (`useInboxPoller.ts:251-364`):
//! - **`PermissionRequest`** → route it (deduped by `tool_use_id`,
//!   `:340-345`) to the leader's approval queue, which prompts the human and
//!   replies to the worker via mailbox.
//! - **regular plain-text message** (gap 4b) → surface to the leader's model
//!   as a coordinator-framed entry on the [`coco_query::CommandQueue`]
//!   (`QueueOrigin::Coordinator`), drained into the leader's next turn. This
//!   is the teammate→leader content path for BOTH in-process and
//!   cross-process teammates (both write to the team-lead mailbox).
//! - **`IdleNotification`** (gap 4b) → same path, formatted as a teammate
//!   status line so the leader learns the worker finished / went idle.
//!
//! coco-rs differences (forced by the layer split):
//! - Runtime team source is the coordinator roster via
//!   [`coco_tool_runtime::AgentHandle::active_team_name`], not
//!   `appState.teamContext` — `team_context` lives on the TUI-only
//!   `AppState`, unreachable from the engine/tool-shared `ToolAppState`.
//! - The approval queue is the registered
//!   [`crate::leader_permission`] setter (→ `TuiPermissionBridge` →
//!   `send_permission_response_via_mailbox`); it carries the human-UI +
//!   reply that TS does inline in the hook.
//!
//! Worker-side responses are NOT handled here: the worker's
//! `MailboxPermissionBridge` polls its own inbox for the reply (TS routes
//! those via `useSwarmPermissionPoller`). Plan-approval / team-permission /
//! mode-set / shutdown / sandbox message types stay on their existing paths
//! (left unread for their dedicated consumers).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use coco_coordinator::mailbox;
use coco_query::CommandQueue;
use coco_query::QueuePriority;
use coco_query::QueuedCommand;
use coco_system_reminder::QueueOrigin;

use crate::session_runtime::SessionRuntime;

/// TS `useInboxPoller.ts:107` `INBOX_POLL_INTERVAL_MS = 1000`.
const INBOX_POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Canonical leader inbox name. TS `TEAM_LEAD_NAME = 'team-lead'`.
const TEAM_LEAD_NAME: &str = "team-lead";

/// Spawn the continuous leader inbox poller. Returns the `JoinHandle` the
/// caller holds for the session lifetime (drop / abort stops it). No-ops
/// each tick until the session has an active team (post-`TeamCreate`) and a
/// registered leader approval queue.
pub fn spawn(runtime: Arc<SessionRuntime>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // tool_use_ids already dispatched to the leader UI — dedup so a
        // failed mark-read on a prior tick doesn't re-prompt the human.
        // TS dedups the ToolUseConfirm queue by tool_use_id (:340-345).
        let mut dispatched: HashSet<String> = HashSet::new();
        loop {
            poll_once(&runtime, &mut dispatched).await;
            tokio::time::sleep(INBOX_POLL_INTERVAL).await;
        }
    })
}

async fn poll_once(runtime: &SessionRuntime, dispatched: &mut HashSet<String>) {
    // Resolve the active team from the roster (TS reads
    // `appState.teamContext?.teamName` each tick, `:143-150`).
    let Some(handle) = runtime.current_agent_handle().await else {
        return;
    };
    let Some(team) = handle.active_team_name().await else {
        return;
    };
    // Optional: regular/idle messages surface to the model via the command
    // queue and don't need an approval UI; only `PermissionRequest` does.
    let permission_setter = coco_coordinator::teammate::get_leader_permission_queue().await;
    let queue = runtime.command_queue();

    let messages = mailbox::read_mailbox(TEAM_LEAD_NAME, &team).unwrap_or_default();
    for (idx, m) in messages.iter().enumerate() {
        if m.read {
            continue;
        }

        // Plain-text teammate message → surface to the leader's model as a
        // coordinator-framed queued command (gap 4b). Self-describing via the
        // `<teammate_message teammate_id=…>` wrapper; drained at the leader's
        // next turn.
        if !mailbox::is_structured_protocol_message(&m.text) {
            enqueue_coordinator_message(
                queue,
                mailbox::format_teammate_messages(std::slice::from_ref(m)),
            )
            .await;
            let _ = mailbox::mark_message_as_read_by_index(TEAM_LEAD_NAME, &team, idx);
            continue;
        }

        let Some(parsed) = mailbox::parse_protocol_message(&m.text) else {
            continue;
        };
        match &parsed {
            mailbox::ProtocolMessage::PermissionRequest { tool_use_id, .. } => {
                // Needs the approval UI. Absent (not a leader TUI) → leave
                // unread; the worker's bounded wait fails closed on timeout.
                let Some(setter) = permission_setter.clone() else {
                    continue;
                };
                // Dispatch once per tool_use_id; the leader setter prompts the
                // human and replies to the worker.
                if dispatched.insert(tool_use_id.clone()) {
                    setter(serde_json::to_value(&parsed).unwrap_or_default());
                }
                let _ = mailbox::mark_message_as_read_by_index(TEAM_LEAD_NAME, &team, idx);
            }
            mailbox::ProtocolMessage::IdleNotification { .. } => {
                enqueue_coordinator_message(queue, format_idle_notification(&parsed)).await;
                let _ = mailbox::mark_message_as_read_by_index(TEAM_LEAD_NAME, &team, idx);
            }
            // Plan-approval / shutdown / mode-set / team-permission / sandbox
            // stay on their existing consumers — leave unread.
            _ => {}
        }
    }
}

/// Enqueue a teammate-originated message onto the leader's mid-turn command
/// queue with `QueueOrigin::Coordinator` framing (TS `wrapCommandText`'s
/// "teammate routed a message via the swarm coordinator" case). Drained into
/// the leader's next turn as a `queued_command` attachment. `Later` priority
/// so it never jumps ahead of the human's own queued input.
async fn enqueue_coordinator_message(queue: &CommandQueue, content: String) {
    if content.trim().is_empty() {
        return;
    }
    let cmd =
        QueuedCommand::new(content, QueuePriority::Later).with_origin(QueueOrigin::Coordinator);
    queue.enqueue(cmd).await;
}

/// Render an `IdleNotification` as a teammate-attributed status line so the
/// leader learns a worker finished its task / went idle. Wrapped in the
/// `<teammate_message teammate_id=…>` envelope for sender attribution.
fn format_idle_notification(parsed: &mailbox::ProtocolMessage) -> String {
    let mailbox::ProtocolMessage::IdleNotification {
        from,
        idle_reason,
        summary,
        completed_task_id,
        completed_status,
        failure_reason,
        ..
    } = parsed
    else {
        return String::new();
    };
    let mut text = String::from("is now idle and available");
    if let Some(reason) = idle_reason {
        text.push_str(&format!(" ({reason})"));
    }
    if let Some(task_id) = completed_task_id {
        let status = completed_status.as_deref().unwrap_or("done");
        text.push_str(&format!("; completed task {task_id} ({status})"));
    }
    if let Some(reason) = failure_reason {
        text.push_str(&format!("; failure: {reason}"));
    }
    let synthetic = mailbox::TeammateMessage {
        from: from.clone(),
        text,
        timestamp: String::new(),
        read: false,
        color: None,
        summary: summary.clone(),
    };
    mailbox::format_teammate_messages(std::slice::from_ref(&synthetic))
}

#[cfg(test)]
#[path = "leader_inbox_poller.test.rs"]
mod tests;
