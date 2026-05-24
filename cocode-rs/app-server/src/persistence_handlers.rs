//! Handlers for session persistence, config, and rewind operations.
//!
//! Extracted from `processor.rs` to keep modules under 500 LoC.

use std::sync::Arc;

use cocode_app_server_protocol::ConfigReadResult;
use cocode_app_server_protocol::ConfigWriteScope;
use cocode_app_server_protocol::RewindFilesRequestParams;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::SessionListRequestParams;
use cocode_app_server_protocol::SessionListResult;
use cocode_app_server_protocol::SessionResumeRequestParams;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_file_backup::RewindMode;
use cocode_session::SessionManager;
use cocode_session::SessionState;
use tracing::info;
use tracing::warn;

use crate::session_factory::SessionHandle;

/// List saved sessions from the storage directory with pagination.
pub async fn handle_session_list(params: &SessionListRequestParams) -> SessionListResult {
    let limit = params.limit.unwrap_or(50).max(1) as usize;
    let manager = SessionManager::new();

    let summaries = match manager.list_persisted().await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Failed to list persisted sessions");
            return SessionListResult {
                sessions: vec![],
                next_cursor: None,
            };
        }
    };

    // Apply cursor-based pagination: skip until we pass the cursor ID
    let start_idx = if let Some(ref cursor) = params.cursor {
        summaries
            .iter()
            .position(|s| s.id == *cursor)
            .map(|i| i + 1)
            .unwrap_or(0)
    } else {
        0
    };

    let page: Vec<_> = summaries
        .into_iter()
        .skip(start_idx)
        .take(limit + 1)
        .collect();

    let has_more = page.len() > limit;
    let items: Vec<_> = page.into_iter().take(limit).collect();
    let next_cursor = if has_more {
        items.last().map(|s| s.id.clone())
    } else {
        None
    };

    let sessions = items
        .into_iter()
        .map(|s| cocode_app_server_protocol::SessionSummary {
            id: s.id,
            name: s.name,
            working_dir: None,
            model: Some(s.model),
            created_at: Some(s.created_at),
            updated_at: Some(s.last_activity_at),
            turn_count: s.turn_count,
        })
        .collect();

    SessionListResult {
        sessions,
        next_cursor,
    }
}

/// Read a session's conversation items from disk (without resuming).
///
/// Returns a JSON value with `items` (array of simplified turn records)
/// and `session` metadata.
pub async fn handle_session_read(session_id: &str) -> serde_json::Value {
    let path = cocode_session::persistence::session_file_path(session_id);

    let (session, history, _snapshots) = match cocode_session::load_session_from_file(&path).await {
        Ok(data) => data,
        Err(e) => {
            warn!(session_id, error = %e, "Failed to load session for read");
            return serde_json::json!({
                "error": format!("Session not found: {e}"),
                "items": [],
            });
        }
    };

    let items: Vec<serde_json::Value> = history
        .turns()
        .iter()
        .map(|turn| {
            let tool_calls: Vec<serde_json::Value> = turn
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "tool": tc.name,
                        "status": format!("{:?}", tc.status),
                    })
                })
                .collect();

            let assistant_text = turn
                .assistant_message
                .as_ref()
                .map(cocode_message::TrackedMessage::text)
                .unwrap_or_default();

            serde_json::json!({
                "turn_id": turn.id,
                "turn_number": turn.number,
                "user_message": turn.user_message.text(),
                "assistant_message": assistant_text,
                "tool_calls": tool_calls,
            })
        })
        .collect();

    serde_json::json!({
        "session": {
            "id": session.id,
            "title": session.title,
            "model": session.model(),
            "provider": session.provider(),
            "created_at": session.created_at.to_rfc3339(),
            "updated_at": session.last_activity_at.to_rfc3339(),
        },
        "items": items,
    })
}

/// Archive a session by deleting its persistence file.
pub async fn handle_session_archive(session_id: &str) -> serde_json::Value {
    let path = cocode_session::persistence::session_file_path(session_id);

    if !cocode_session::persistence::session_file_path(session_id).exists() {
        return serde_json::json!({
            "status": "not_found",
            "error": format!("Session '{session_id}' not found"),
        });
    }

    match tokio::fs::remove_file(&path).await {
        Ok(()) => {
            info!(session_id, "Session archived (deleted)");
            // Also remove the backup directory if it exists
            let backup_dir = cocode_session::persistence::default_sessions_dir().join(session_id);
            if backup_dir.exists() {
                let _ = tokio::fs::remove_dir_all(&backup_dir).await;
            }
            serde_json::json!({"status": "ok"})
        }
        Err(e) => {
            warn!(session_id, error = %e, "Failed to archive session");
            serde_json::json!({
                "status": "error",
                "error": format!("Failed to archive: {e}"),
            })
        }
    }
}

/// Resume an existing session, returning a new SessionHandle.
pub async fn handle_session_resume(
    config: &ConfigManager,
    params: &SessionResumeRequestParams,
) -> Result<SessionHandle, String> {
    let path = cocode_session::persistence::session_file_path(&params.session_id);

    let (session, history, snapshots) = cocode_session::load_session_from_file(&path)
        .await
        .map_err(|e| format!("Failed to load session '{}': {e}", params.session_id))?;

    let working_dir = session.working_dir.clone();
    let overrides = ConfigOverrides::default().with_cwd(working_dir);
    let snapshot = Arc::new(
        config
            .build_config(overrides)
            .map_err(|e| format!("Failed to build config: {e}"))?,
    );

    let mut state = SessionState::new(session, snapshot)
        .await
        .map_err(|e| format!("Failed to create session state: {e}"))?;

    // Restore message history from the persisted session
    state.message_history = history;

    // Restore snapshot stack for rewind support
    if !snapshots.is_empty()
        && let Some(sm) = state.snapshot_manager()
    {
        let json = serde_json::to_string(&snapshots)
            .map_err(|e| format!("Failed to serialize snapshots: {e}"))?;
        if let Err(e) = sm.restore_snapshots(&json).await {
            warn!(error = %e, "Failed to restore snapshots on resume");
        }
    }

    info!(
        session_id = %params.session_id,
        turns = state.total_turns(),
        "Session resumed"
    );

    Ok(SessionHandle {
        state,
        hook_bridge: None,
        mcp_bridge: None,
        permission_bridge: None,
        turn_number: 0,
    })
}

/// Read effective configuration, optionally filtered by key.
pub fn handle_config_read(config: &ConfigManager, key: Option<&str>) -> ConfigReadResult {
    let app_config = config.app_config();

    let config_json = serde_json::to_value(&app_config).unwrap_or_default();

    let value = if let Some(key) = key {
        extract_nested_value(&config_json, key)
    } else {
        config_json
    };

    ConfigReadResult { config: value }
}

/// Write a configuration value to the user or project config file.
pub async fn handle_config_write(
    config: &ConfigManager,
    key: &str,
    value: serde_json::Value,
    scope: ConfigWriteScope,
) -> Result<(), String> {
    let config_dir = match scope {
        ConfigWriteScope::User => cocode_config::find_cocode_home(),
        ConfigWriteScope::Project => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            cwd.join(".cocode")
        }
    };

    let config_path = config_dir.join("config.json");

    // Load existing config or start fresh
    let mut existing: serde_json::Value = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| format!("Failed to read config: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {e}"))?
    } else {
        serde_json::json!({})
    };

    // Set the nested value using dot-separated key path
    set_nested_value(&mut existing, key, value);

    // Write back
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create config dir: {e}"))?;
    }

    let content = serde_json::to_string_pretty(&existing)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;

    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| format!("Failed to write config: {e}"))?;

    // Reload the in-memory config
    if let Err(e) = config.reload() {
        warn!(error = %e, "Config reload after write failed");
    }

    info!(key, scope = ?scope, "Config value written");
    Ok(())
}

/// Rewind files to a previous turn's state.
///
/// Uses the snapshot manager from the active session to restore files,
/// then rewinds conversation state in the message history.
pub async fn handle_rewind_files(
    state: &mut SessionState,
    params: &RewindFilesRequestParams,
) -> ServerNotification {
    let turn_number: i32 = match params.turn_id.parse() {
        Ok(n) => n,
        Err(_) => {
            // Try parsing "turn_N" format
            params
                .turn_id
                .strip_prefix("turn_")
                .and_then(|s| s.parse().ok())
                .unwrap_or(-1)
        }
    };

    if turn_number < 0 {
        return ServerNotification::RewindFailed(cocode_app_server_protocol::RewindFailedParams {
            error: format!("Invalid turn_id: '{}'", params.turn_id),
        });
    }

    // First, rewind files via the snapshot manager
    let file_result = if let Some(sm) = state.snapshot_manager() {
        match sm
            .rewind_to_turn_with_mode(Some(turn_number), RewindMode::CodeAndConversation)
            .await
        {
            Ok(result) => Some(result),
            Err(e) => {
                return ServerNotification::RewindFailed(
                    cocode_app_server_protocol::RewindFailedParams {
                        error: format!("File rewind failed: {e}"),
                    },
                );
            }
        }
    } else {
        None
    };

    // Then rewind conversation state
    let (messages_removed, _restored_prompt) =
        state.rewind_conversation_state_from_turn(turn_number);

    let restored_files = file_result
        .as_ref()
        .map(|r| r.restored_files.len() as i32)
        .unwrap_or(0);

    info!(
        turn = turn_number,
        restored_files, messages_removed, "Rewind completed"
    );

    ServerNotification::RewindCompleted(cocode_app_server_protocol::RewindCompletedParams {
        rewound_turn: turn_number,
        restored_files,
        messages_removed,
    })
}

/// Extract a nested value from a JSON object using dot-separated key path.
fn extract_nested_value(root: &serde_json::Value, key: &str) -> serde_json::Value {
    let mut current = root;
    for part in key.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return serde_json::Value::Null,
        }
    }
    current.clone()
}

/// Set a nested value in a JSON object using dot-separated key path.
fn set_nested_value(root: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last segment: set the value
            if let serde_json::Value::Object(map) = current {
                map.insert((*part).to_string(), value);
            }
            return;
        }
        // Intermediate segment: ensure it's an object
        if !current.get(*part).is_some_and(serde_json::Value::is_object)
            && let serde_json::Value::Object(map) = current
        {
            map.insert((*part).to_string(), serde_json::json!({}));
        }
        let Some(next) = current.get_mut(*part) else {
            return;
        };
        current = next;
    }
}

#[cfg(test)]
#[path = "persistence_handlers.test.rs"]
mod tests;
