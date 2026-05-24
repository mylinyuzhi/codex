//! Post-compact async-agent attachments.
//!
//! TS: `createAsyncAgentAttachmentsIfNeeded()` in `compact.ts:1568-1599`.
//! After compaction wipes the visible conversation, the model loses
//! knowledge of running background agents and may spawn duplicates. This
//! helper emits one `task_status` attachment per running / finished-but-
//! unretrieved local-agent task so the post-compact prompt carries that
//! state on the FIRST post-compact turn.
//!
//! coco-rs caller (engine_compaction.rs) takes a snapshot of the running
//! `TaskManager` at compact time and passes the filtered task list in. This
//! crate stays free of `coco-tasks` deps.

use coco_messages::AttachmentMessage;
use coco_messages::LlmMessage;

/// Snapshot of one async-agent task as captured by the engine at compact
/// time, before the attachment callback runs. Mirrors the relevant fields
/// from TS `LocalAgentTaskState`.
#[derive(Debug, Clone)]
pub struct AsyncAgentSnapshot {
    /// Task id (TS `taskId` / `agentId`).
    pub task_id: String,
    /// Status string suitable for inline rendering. TS uses the
    /// `LocalAgentTaskState.status` enum (`'running' | 'completed' | …`);
    /// coco-rs passes the lowercase `TaskStatus.as_str()` form.
    pub status: String,
    /// Human-readable description supplied at task creation.
    pub description: String,
    /// Optional summary delta — `progress.summary` for running tasks,
    /// `error` for failed ones. TS `compact.ts:1591-1593`.
    pub delta_summary: Option<String>,
    /// Filesystem path to the persisted task output. TS `getTaskOutputPath`.
    pub output_file_path: String,
}

/// Build one `task_status` post-compact attachment per snapshotted async
/// agent. Returns an empty `Vec` when no agents qualify.
///
/// Caller is responsible for filtering — TS rules
/// (`compact.ts:1577-1582`):
///   - skip `retrieved == true` (model already saw the result),
///   - skip `status == 'pending'` (not yet meaningful),
///   - skip the agent owned by the current sub-agent itself.
pub fn create_async_agent_attachments(snapshots: &[AsyncAgentSnapshot]) -> Vec<AttachmentMessage> {
    snapshots.iter().map(render_one).collect()
}

fn render_one(snap: &AsyncAgentSnapshot) -> AttachmentMessage {
    // TS template: rendered by `normalizeAttachmentForAPI` for the
    // `task_status` discriminator. The exact text matters for cache
    // stability — keep close to TS: a header line plus optional summary
    // plus the persisted output path so the model can fetch the full
    // transcript via `Read`.
    let mut body = format!(
        "Background agent {task_id} status: {status}\nDescription: {desc}",
        task_id = snap.task_id,
        status = snap.status,
        desc = snap.description,
    );
    if let Some(delta) = snap.delta_summary.as_deref()
        && !delta.is_empty()
    {
        body.push_str("\nSummary: ");
        body.push_str(delta);
    }
    if !snap.output_file_path.is_empty() {
        body.push_str("\nOutput file: ");
        body.push_str(&snap.output_file_path);
    }
    AttachmentMessage::api(
        coco_types::AttachmentKind::TaskStatus,
        LlmMessage::user_text(coco_messages::wrapping::wrap_in_system_reminder(&body)),
    )
}

#[cfg(test)]
#[path = "post_compact_async_agents.test.rs"]
mod tests;
