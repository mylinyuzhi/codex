//! File-based mailbox system for inter-teammate messaging.
//!
//! TS: utils/teammateMailbox.ts
//!
//! Inbox location: `~/.claude/teams/{team_name}/inboxes/{agent_name}.json`
//! Uses file locking for concurrent access safety.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use super::swarm_file_io::get_team_dir;

// ── Core Message Type ──

/// A message in the teammate mailbox.
///
/// TS: `TeammateMessage` in utils/teammateMailbox.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateMessage {
    pub from: String,
    pub text: String,
    pub timestamp: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Brief preview (5-10 words).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ── Inbox Path ──

/// Get the inbox directory for a team.
fn inbox_dir(team_name: &str) -> PathBuf {
    get_team_dir(team_name).join("inboxes")
}

/// Get the inbox file path for a specific agent.
///
/// TS: `getInboxPath(agentName, teamName)`
pub fn inbox_path(agent_name: &str, team_name: &str) -> PathBuf {
    inbox_dir(team_name).join(format!("{agent_name}.json"))
}

// ── Read / Write ──

/// Read all messages from an agent's inbox.
///
/// TS: `readMailbox(agentName, teamName)`
pub fn read_mailbox(agent_name: &str, team_name: &str) -> anyhow::Result<Vec<TeammateMessage>> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let messages: Vec<TeammateMessage> = serde_json::from_str(&content)?;
    Ok(messages)
}

/// Read only unread messages.
///
/// TS: `readUnreadMessages(agentName, teamName)`
pub fn read_unread_messages(
    agent_name: &str,
    team_name: &str,
) -> anyhow::Result<Vec<TeammateMessage>> {
    let all = read_mailbox(agent_name, team_name)?;
    Ok(all.into_iter().filter(|m| !m.read).collect())
}

/// Write a message to a recipient's inbox.
///
/// TS: `writeToMailbox(recipientName, message, teamName)` — uses
/// `proper-lockfile` with 10 retries and 5-100ms exponential backoff.
/// We mirror that with `fs2`'s advisory exclusive lock on a sidecar
/// `.lock` file; concurrent writers spin with backoff until they acquire
/// it. Read-after-lock prevents the classic TOCTOU of "read-mailbox →
/// append → write" losing a concurrent peer's message.
pub fn write_to_mailbox(
    recipient_name: &str,
    message: TeammateMessage,
    team_name: &str,
) -> anyhow::Result<()> {
    let path = inbox_path(recipient_name, team_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    with_inbox_lock(&path, |path| {
        // Inside the lock: read-current, append, write. The outer lock
        // serializes this RMW cycle against concurrent writers.
        let mut messages = read_messages_no_lock(path).unwrap_or_default();
        messages.push(message.clone());
        let content = serde_json::to_string_pretty(&messages)?;
        std::fs::write(path, content)?;
        Ok(())
    })
}

/// Run `body` while holding an exclusive advisory lock on a sidecar
/// `{path}.lock` file, retrying acquisition on contention.
///
/// 10 retries with 5→100ms backoff (mirrors TS `proper-lockfile`
/// defaults). After exhaustion we bubble the error so the caller can
/// surface a clear failure rather than silently dropping the message.
fn with_inbox_lock<F>(path: &std::path::Path, body: F) -> anyhow::Result<()>
where
    F: FnOnce(&std::path::Path) -> anyhow::Result<()>,
{
    use fs2::FileExt;
    let lock_path = path.with_extension("json.lock");
    // `create(true)` so the lockfile can be created on first access to
    // an inbox that doesn't yet exist. We don't write anything into it;
    // it's purely the lock target.
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)?;

    // 30 retries × up to 50ms = ~1.5s upper bound under contention.
    // Jitter scales each backoff by [0.5, 1.5) to break thundering-herd
    // wakeups when many writers contend at once (pure exponential
    // backoff syncs all retry-waves across threads).
    const MAX_RETRIES: u32 = 30;
    let mut delay_ms: u64 = 2;
    for attempt in 0..MAX_RETRIES {
        match lock_file.try_lock_exclusive() {
            Ok(()) => {
                let result = body(path);
                let _ = lock_file.unlock();
                return result;
            }
            Err(_) if attempt + 1 < MAX_RETRIES => {
                let jitter_bits = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                let jitter_pct = 50 + (jitter_bits % 100) as u64; // [50, 150)
                let jittered = delay_ms * jitter_pct / 100;
                std::thread::sleep(std::time::Duration::from_millis(jittered.max(1)));
                delay_ms = (delay_ms * 2).min(50);
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to acquire mailbox lock at {} after {MAX_RETRIES} retries: {e}",
                    lock_path.display()
                ));
            }
        }
    }
    unreachable!("retry loop always returns")
}

/// Read the mailbox file without acquiring the lock. Callers that need
/// lock-protected read-modify-write should use this from inside
/// `with_inbox_lock` to avoid recursive locking. Callers that just want
/// a point-in-time read can use the public [`read_mailbox`] which is
/// lock-free (readers accept slight staleness).
fn read_messages_no_lock(path: &std::path::Path) -> anyhow::Result<Vec<TeammateMessage>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

/// Mark all messages as read.
///
/// TS: `markMessagesAsRead(agentName, teamName)`
pub fn mark_messages_as_read(agent_name: &str, team_name: &str) -> anyhow::Result<()> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(());
    }
    let mut messages = read_mailbox(agent_name, team_name)?;
    for msg in &mut messages {
        msg.read = true;
    }
    let content = serde_json::to_string_pretty(&messages)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Mark a message as read by index.
///
/// TS: `markMessageAsReadByIndex(agentName, teamName, messageIndex)`
pub fn mark_message_as_read_by_index(
    agent_name: &str,
    team_name: &str,
    index: usize,
) -> anyhow::Result<()> {
    let path = inbox_path(agent_name, team_name);
    let mut messages = read_mailbox(agent_name, team_name)?;
    if let Some(msg) = messages.get_mut(index) {
        msg.read = true;
        let content = serde_json::to_string_pretty(&messages)?;
        std::fs::write(&path, content)?;
    }
    Ok(())
}

/// Mark messages as read by predicate.
///
/// TS: `markMessagesAsReadByPredicate(agentName, predicate, teamName?)`
pub fn mark_messages_as_read_by_predicate(
    agent_name: &str,
    team_name: &str,
    predicate: impl Fn(&TeammateMessage) -> bool,
) -> anyhow::Result<()> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(());
    }
    let mut messages = read_mailbox(agent_name, team_name)?;
    for msg in &mut messages {
        if predicate(msg) {
            msg.read = true;
        }
    }
    let content = serde_json::to_string_pretty(&messages)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Clear an agent's inbox.
///
/// TS: `clearMailbox(agentName, teamName)`
pub fn clear_mailbox(agent_name: &str, team_name: &str) -> anyhow::Result<()> {
    let path = inbox_path(agent_name, team_name);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Format teammate messages for display in the conversation.
///
/// TS: `formatTeammateMessages(messages)`
pub fn format_teammate_messages(messages: &[TeammateMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let color_attr = m
                .color
                .as_deref()
                .map(|c| format!(" color=\"{c}\""))
                .unwrap_or_default();
            let summary_attr = m
                .summary
                .as_deref()
                .map(|s| format!(" summary=\"{s}\""))
                .unwrap_or_default();
            format!(
                "<teammate_message teammate_id=\"{from}\"{color_attr}{summary_attr}>\n{text}\n</teammate_message>",
                from = m.from,
                text = m.text,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── Protocol Message Helpers ──

/// Check if a message text is a structured protocol message.
///
/// TS: `isStructuredProtocolMessage(messageText)`
pub fn is_structured_protocol_message(text: &str) -> bool {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };
    v.get("type").and_then(|t| t.as_str()).is_some_and(|t| {
        matches!(
            t,
            "idle_notification"
                | "permission_request"
                | "permission_response"
                | "sandbox_permission_request"
                | "sandbox_permission_response"
                | "plan_approval_request"
                | "plan_approval_response"
                | "shutdown_request"
                | "shutdown_approved"
                | "shutdown_rejected"
                | "task_assignment"
                | "team_permission_update"
                | "mode_set_request"
        )
    })
}

/// Parse a structured protocol message from text.
pub fn parse_protocol_message(text: &str) -> Option<ProtocolMessage> {
    let trimmed = text.trim();
    serde_json::from_str(trimmed).ok()
}

/// All structured protocol message types.
///
/// TS: various type definitions in utils/teammateMailbox.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    IdleNotification {
        from: String,
        timestamp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure_reason: Option<String>,
    },
    PermissionRequest {
        request_id: String,
        agent_id: String,
        tool_name: String,
        tool_use_id: String,
        description: String,
        #[serde(default)]
        input: serde_json::Value,
        /// TS: `permission_suggestions: unknown[]`
        #[serde(default)]
        permission_suggestions: Vec<serde_json::Value>,
    },
    PermissionResponse {
        request_id: String,
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SandboxPermissionRequest {
        request_id: String,
        worker_id: String,
        worker_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_color: Option<String>,
        host_pattern: serde_json::Value,
        created_at: i64,
    },
    SandboxPermissionResponse {
        request_id: String,
        host: String,
        allow: bool,
        timestamp: String,
    },
    PlanApprovalRequest {
        from: String,
        timestamp: String,
        plan_file_path: String,
        plan_content: String,
        request_id: String,
    },
    PlanApprovalResponse {
        request_id: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
        timestamp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
    },
    ShutdownRequest {
        request_id: String,
        from: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        timestamp: String,
    },
    ShutdownApproved {
        request_id: String,
        from: String,
        timestamp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backend_type: Option<String>,
    },
    ShutdownRejected {
        request_id: String,
        from: String,
        reason: String,
        timestamp: String,
    },
    TaskAssignment {
        task_id: String,
        subject: String,
        description: String,
        assigned_by: String,
        timestamp: String,
    },
    TeamPermissionUpdate {
        permission_update: serde_json::Value,
        directory_path: String,
        tool_name: String,
    },
    ModeSetRequest {
        mode: String,
        from: String,
    },
}

/// Send a shutdown request to an agent's mailbox.
///
/// TS: `sendShutdownRequestToMailbox(targetName, teamName, reason)`
pub fn send_shutdown_request(
    target_name: &str,
    team_name: &str,
    from: &str,
    reason: Option<&str>,
) -> anyhow::Result<String> {
    let request_id = format!("shutdown-{}", uuid::Uuid::new_v4());
    let timestamp = chrono::Utc::now().to_rfc3339();

    let protocol = ProtocolMessage::ShutdownRequest {
        request_id: request_id.clone(),
        from: from.to_string(),
        reason: reason.map(String::from),
        timestamp: timestamp.clone(),
    };
    let text = serde_json::to_string(&protocol)?;

    let message = TeammateMessage {
        from: from.to_string(),
        text,
        timestamp,
        read: false,
        color: None,
        summary: Some("shutdown request".to_string()),
    };

    write_to_mailbox(target_name, message, team_name)?;
    Ok(request_id)
}

/// Create an idle notification protocol message.
///
/// TS: `createIdleNotification(agentId, options)`
pub fn create_idle_notification(
    from: &str,
    idle_reason: Option<&str>,
    summary: Option<&str>,
) -> String {
    let msg = ProtocolMessage::IdleNotification {
        from: from.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        idle_reason: idle_reason.map(String::from),
        summary: summary.map(String::from),
        completed_task_id: None,
        completed_status: None,
        failure_reason: None,
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

/// Create a mode set request protocol message.
///
/// TS: `createModeSetRequestMessage(params)`
pub fn create_mode_set_request(mode: &str, from: &str) -> String {
    let msg = ProtocolMessage::ModeSetRequest {
        mode: mode.to_string(),
        from: from.to_string(),
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

// ── Permission Sync Directories ──

/// Get the permissions directory for a team.
pub fn permissions_dir(team_name: &str) -> PathBuf {
    get_team_dir(team_name).join("permissions")
}

/// Get the pending permissions directory.
pub fn pending_permissions_dir(team_name: &str) -> PathBuf {
    permissions_dir(team_name).join("pending")
}

/// Get the resolved permissions directory.
pub fn resolved_permissions_dir(team_name: &str) -> PathBuf {
    permissions_dir(team_name).join("resolved")
}

/// Ensure permission directories exist.
pub fn ensure_permission_dirs(team_name: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(pending_permissions_dir(team_name))?;
    std::fs::create_dir_all(resolved_permissions_dir(team_name))?;
    Ok(())
}

/// Write a permission request to the pending directory.
pub fn write_pending_permission(
    team_name: &str,
    request: &super::swarm::SwarmPermissionRequest,
) -> anyhow::Result<()> {
    ensure_permission_dirs(team_name)?;
    let path = pending_permissions_dir(team_name).join(format!("{}.json", request.id));
    let content = serde_json::to_string_pretty(request)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Read and remove a resolved permission response.
pub fn read_resolved_permission(
    team_name: &str,
    request_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let path = resolved_permissions_dir(team_name).join(format!("{request_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&content)?;
    std::fs::remove_file(&path)?;
    Ok(Some(value))
}

/// Read all pending permission requests.
///
/// TS: `readPendingPermissions(teamName?)`
pub fn read_pending_permissions(
    team_name: &str,
) -> anyhow::Result<Vec<super::swarm::SwarmPermissionRequest>> {
    let dir = pending_permissions_dir(team_name);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut requests = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        if entry.path().extension().is_some_and(|e| e == "json")
            && let Ok(content) = std::fs::read_to_string(entry.path())
            && let Ok(req) = serde_json::from_str(&content)
        {
            requests.push(req);
        }
    }
    Ok(requests)
}

/// Resolve a pending permission (move from pending/ to resolved/).
///
/// TS: `resolvePermission(requestId, resolution, teamName?)`
pub fn resolve_permission(
    team_name: &str,
    request_id: &str,
    resolution: &serde_json::Value,
) -> anyhow::Result<()> {
    ensure_permission_dirs(team_name)?;
    let pending = pending_permissions_dir(team_name).join(format!("{request_id}.json"));
    let resolved = resolved_permissions_dir(team_name).join(format!("{request_id}.json"));
    let content = serde_json::to_string_pretty(resolution)?;
    std::fs::write(&resolved, content)?;
    let _ = std::fs::remove_file(&pending);
    Ok(())
}

/// Clean up old resolved permissions.
///
/// TS: `cleanupOldResolutions(teamName?, maxAgeMs?)`
pub fn cleanup_old_resolutions(team_name: &str, max_age_ms: i64) -> anyhow::Result<()> {
    let dir = resolved_permissions_dir(team_name);
    if !dir.is_dir() {
        return Ok(());
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for entry in std::fs::read_dir(&dir)?.flatten() {
        if let Ok(meta) = entry.metadata()
            && let Ok(modified) = meta.modified()
        {
            let mtime_ms = modified
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if now_ms - mtime_ms > max_age_ms {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

/// Poll for a resolved permission response (non-blocking).
///
/// TS: `pollForResponse(requestId, agentName?, teamName?)`
pub fn poll_for_response(
    team_name: &str,
    request_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    read_resolved_permission(team_name, request_id)
}

/// Remove a worker's resolved response file.
///
/// TS: `removeWorkerResponse(requestId, agentName?, teamName?)`
pub fn remove_worker_response(team_name: &str, request_id: &str) -> anyhow::Result<()> {
    let path = resolved_permissions_dir(team_name).join(format!("{request_id}.json"));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Send a permission request to the leader's mailbox.
///
/// TS: `sendPermissionRequestViaMailbox(request)`
pub fn send_permission_request_via_mailbox(
    request: &super::swarm::SwarmPermissionRequest,
) -> anyhow::Result<()> {
    let text = serde_json::to_string(&ProtocolMessage::PermissionRequest {
        request_id: request.id.clone(),
        agent_id: request.worker_id.clone(),
        tool_name: request.tool_name.clone(),
        tool_use_id: request.tool_use_id.clone(),
        description: request.description.clone(),
        input: request.input.clone(),
        permission_suggestions: Vec::new(),
    })?;
    let message = TeammateMessage {
        from: request.worker_name.clone(),
        text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: request.worker_color.clone(),
        summary: Some("permission request".to_string()),
    };
    write_to_mailbox(
        super::swarm_constants::TEAM_LEAD_NAME,
        message,
        &request.team_name,
    )
}

/// Send a permission response to a worker's mailbox.
///
/// TS: `sendPermissionResponseViaMailbox(workerName, resolution, requestId, teamName?)`
pub fn send_permission_response_via_mailbox(
    worker_name: &str,
    request_id: &str,
    approved: bool,
    feedback: Option<&str>,
    team_name: &str,
) -> anyhow::Result<()> {
    let subtype = if approved { "success" } else { "error" };
    let text = serde_json::to_string(&ProtocolMessage::PermissionResponse {
        request_id: request_id.to_string(),
        subtype: subtype.to_string(),
        response: None,
        error: if approved {
            None
        } else {
            feedback.map(String::from)
        },
    })?;
    let message = TeammateMessage {
        from: super::swarm_constants::TEAM_LEAD_NAME.to_string(),
        text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: None,
        summary: Some("permission response".to_string()),
    };
    write_to_mailbox(worker_name, message, team_name)
}

/// Send a sandbox permission request to the leader's mailbox.
///
/// TS: `sendSandboxPermissionRequestViaMailbox(host, requestId, teamName?)`
pub fn send_sandbox_permission_request_via_mailbox(
    worker_name: &str,
    worker_id: &str,
    request_id: &str,
    host: &str,
    team_name: &str,
) -> anyhow::Result<()> {
    let text = serde_json::to_string(&ProtocolMessage::SandboxPermissionRequest {
        request_id: request_id.to_string(),
        worker_id: worker_id.to_string(),
        worker_name: worker_name.to_string(),
        worker_color: None,
        host_pattern: serde_json::json!({"host": host}),
        created_at: chrono::Utc::now().timestamp_millis(),
    })?;
    let message = TeammateMessage {
        from: worker_name.to_string(),
        text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: None,
        summary: Some("sandbox permission request".to_string()),
    };
    write_to_mailbox(super::swarm_constants::TEAM_LEAD_NAME, message, team_name)
}

/// Send a sandbox permission response to a worker's mailbox.
///
/// TS: `sendSandboxPermissionResponseViaMailbox(workerName, requestId, host, allow, teamName?)`
pub fn send_sandbox_permission_response_via_mailbox(
    worker_name: &str,
    request_id: &str,
    host: &str,
    allow: bool,
    team_name: &str,
) -> anyhow::Result<()> {
    let text = serde_json::to_string(&ProtocolMessage::SandboxPermissionResponse {
        request_id: request_id.to_string(),
        host: host.to_string(),
        allow,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })?;
    let message = TeammateMessage {
        from: super::swarm_constants::TEAM_LEAD_NAME.to_string(),
        text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: None,
        summary: Some("sandbox permission response".to_string()),
    };
    write_to_mailbox(worker_name, message, team_name)
}

// ── Protocol Message Creators ──

/// Create a permission request protocol message string.
///
/// TS: `createPermissionRequestMessage(params)`
pub fn create_permission_request_message(
    request_id: &str,
    agent_id: &str,
    tool_name: &str,
    tool_use_id: &str,
    description: &str,
    input: &serde_json::Value,
) -> String {
    let msg = ProtocolMessage::PermissionRequest {
        request_id: request_id.to_string(),
        agent_id: agent_id.to_string(),
        tool_name: tool_name.to_string(),
        tool_use_id: tool_use_id.to_string(),
        description: description.to_string(),
        input: input.clone(),
        permission_suggestions: Vec::new(),
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

/// Create a permission response protocol message string.
///
/// TS: `createPermissionResponseMessage(params)`
pub fn create_permission_response_message(
    request_id: &str,
    approved: bool,
    feedback: Option<&str>,
) -> String {
    let msg = ProtocolMessage::PermissionResponse {
        request_id: request_id.to_string(),
        subtype: if approved { "success" } else { "error" }.to_string(),
        response: None,
        error: if approved {
            None
        } else {
            feedback.map(String::from)
        },
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

/// Create a shutdown approved protocol message string.
///
/// TS: `createShutdownApprovedMessage(params)`
pub fn create_shutdown_approved_message(request_id: &str, from: &str) -> String {
    let msg = ProtocolMessage::ShutdownApproved {
        request_id: request_id.to_string(),
        from: from.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        pane_id: None,
        backend_type: None,
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

/// Create a shutdown rejected protocol message string.
///
/// TS: `createShutdownRejectedMessage(params)`
pub fn create_shutdown_rejected_message(request_id: &str, from: &str, reason: &str) -> String {
    let msg = ProtocolMessage::ShutdownRejected {
        request_id: request_id.to_string(),
        from: from.to_string(),
        reason: reason.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

// ── Protocol Message Type Checkers ──

/// Check if a message is a specific protocol type. Returns typed variant if match.
impl ProtocolMessage {
    /// TS: `isIdleNotification(text)`
    pub fn is_idle_notification(&self) -> bool {
        matches!(self, Self::IdleNotification { .. })
    }
    /// TS: `isPermissionRequest(text)`
    pub fn is_permission_request(&self) -> bool {
        matches!(self, Self::PermissionRequest { .. })
    }
    /// TS: `isPermissionResponse(text)`
    pub fn is_permission_response(&self) -> bool {
        matches!(self, Self::PermissionResponse { .. })
    }
    /// TS: `isShutdownRequest(text)`
    pub fn is_shutdown_request(&self) -> bool {
        matches!(self, Self::ShutdownRequest { .. })
    }
    /// TS: `isShutdownApproved(text)`
    pub fn is_shutdown_approved(&self) -> bool {
        matches!(self, Self::ShutdownApproved { .. })
    }
    /// TS: `isShutdownRejected(text)`
    pub fn is_shutdown_rejected(&self) -> bool {
        matches!(self, Self::ShutdownRejected { .. })
    }
    /// TS: `isTaskAssignment(text)`
    pub fn is_task_assignment(&self) -> bool {
        matches!(self, Self::TaskAssignment { .. })
    }
    /// TS: `isTeamPermissionUpdate(text)`
    pub fn is_team_permission_update(&self) -> bool {
        matches!(self, Self::TeamPermissionUpdate { .. })
    }
    /// TS: `isModeSetRequest(text)`
    pub fn is_mode_set_request(&self) -> bool {
        matches!(self, Self::ModeSetRequest { .. })
    }
    /// TS: `isSandboxPermissionRequest(text)`
    pub fn is_sandbox_permission_request(&self) -> bool {
        matches!(self, Self::SandboxPermissionRequest { .. })
    }
    /// TS: `isSandboxPermissionResponse(text)`
    pub fn is_sandbox_permission_response(&self) -> bool {
        matches!(self, Self::SandboxPermissionResponse { .. })
    }
    /// TS: `isPlanApprovalRequest(text)`
    pub fn is_plan_approval_request(&self) -> bool {
        matches!(self, Self::PlanApprovalRequest { .. })
    }
    /// TS: `isPlanApprovalResponse(text)`
    pub fn is_plan_approval_response(&self) -> bool {
        matches!(self, Self::PlanApprovalResponse { .. })
    }
}

/// Parse a message text and check if it's a specific protocol type.
///
/// Convenience wrapper combining `parse_protocol_message` + type check.
pub fn check_message_type(text: &str, type_name: &str) -> Option<ProtocolMessage> {
    let msg = parse_protocol_message(text)?;
    let matches = match type_name {
        "idle_notification" => msg.is_idle_notification(),
        "permission_request" => msg.is_permission_request(),
        "permission_response" => msg.is_permission_response(),
        "shutdown_request" => msg.is_shutdown_request(),
        "shutdown_approved" => msg.is_shutdown_approved(),
        "shutdown_rejected" => msg.is_shutdown_rejected(),
        "task_assignment" => msg.is_task_assignment(),
        "team_permission_update" => msg.is_team_permission_update(),
        "mode_set_request" => msg.is_mode_set_request(),
        "sandbox_permission_request" => msg.is_sandbox_permission_request(),
        "sandbox_permission_response" => msg.is_sandbox_permission_response(),
        "plan_approval_request" => msg.is_plan_approval_request(),
        "plan_approval_response" => msg.is_plan_approval_response(),
        _ => false,
    };
    if matches { Some(msg) } else { None }
}

// ── Leader/Worker Identity Helpers ──

/// Check if the current agent is the team leader.
///
/// TS: `isTeamLeader(teamName?)`
pub fn is_team_leader(team_name: &str) -> bool {
    let agent_name = super::swarm_identity::get_agent_name();
    agent_name.as_deref() == Some(super::swarm_constants::TEAM_LEAD_NAME)
        || !super::swarm_identity::is_teammate()
        || super::swarm_file_io::read_team_file(team_name)
            .ok()
            .flatten()
            .is_some_and(|tf| {
                super::swarm_identity::get_agent_id().is_some_and(|id| id == tf.lead_agent_id)
            })
}

/// Check if the current agent is a swarm worker.
///
/// TS: `isSwarmWorker()`
pub fn is_swarm_worker() -> bool {
    super::swarm_identity::is_teammate()
}

/// Get the leader's agent name from the team file.
///
/// TS: `getLeaderName(teamName?)`
pub fn get_leader_name(_team_name: &str) -> String {
    super::swarm_constants::TEAM_LEAD_NAME.to_string()
}

// ── Protocol-message helpers (TS parity) ──

/// Generate a deterministic-by-agent-identity-but-unique-per-call
/// request ID for plan_approval.
///
/// TS: `generateRequestId('plan_approval', formatAgentId(agentName, teamName))`
pub fn generate_plan_approval_request_id(agent_name: &str, team_name: &str) -> String {
    // Short random suffix — collisions within a session are astronomically
    // unlikely and the correlation is handled by the leader matching on
    // the full string anyway.
    let rand: String = uuid::Uuid::new_v4().simple().to_string();
    let rand8: String = rand.chars().take(8).collect();
    format!("plan_approval-{agent_name}-{team_name}-{rand8}")
}

// ── MailboxHandle impl for ToolUseContext plumbing (`coco-tool-runtime` trait) ──

/// Concrete `MailboxHandle` implementation that writes via
/// [`write_to_mailbox`]. Engines and spawn paths install one of these
/// on the teammate's `ToolUseContext` so ExitPlanMode + SendMessage can
/// reach the leader's inbox without crossing layer boundaries directly.
#[derive(Debug, Default)]
pub struct SwarmMailboxHandle;

#[async_trait::async_trait]
impl coco_tool_runtime::MailboxHandle for SwarmMailboxHandle {
    async fn write_to_mailbox(
        &self,
        recipient: &str,
        team_name: &str,
        message: coco_tool_runtime::MailboxEnvelope,
    ) -> anyhow::Result<()> {
        let msg = TeammateMessage {
            from: message.from,
            text: message.text,
            timestamp: message.timestamp,
            read: false,
            color: None,
            summary: None,
        };
        write_to_mailbox(recipient, msg, team_name)
    }

    async fn read_unread(
        &self,
        agent_name: &str,
        team_name: &str,
    ) -> anyhow::Result<Vec<coco_tool_runtime::InboxMessage>> {
        // We need indices from the FULL mailbox (to support
        // `mark_read(index)`), so read the full list and filter to
        // unread in-place.
        let all = read_mailbox(agent_name, team_name).unwrap_or_default();
        Ok(all
            .into_iter()
            .enumerate()
            .filter(|(_, m)| !m.read)
            .map(|(index, m)| coco_tool_runtime::InboxMessage {
                index,
                from: m.from,
                text: m.text,
                timestamp: m.timestamp,
            })
            .collect())
    }

    async fn mark_read(
        &self,
        agent_name: &str,
        team_name: &str,
        index: usize,
    ) -> anyhow::Result<()> {
        mark_message_as_read_by_index(agent_name, team_name, index)
    }
}

#[cfg(test)]
#[path = "swarm_mailbox.test.rs"]
mod tests;
