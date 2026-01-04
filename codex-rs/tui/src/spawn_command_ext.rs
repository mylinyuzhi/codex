//! Extension module for /spawn command handling.
//!
//! Provides help text and task list formatting for the TUI /spawn command.
//! Full spawn task management is done via the exec CLI (--iter/--time flags).

use codex_core::spawn_task::SpawnTaskMetadata;
use codex_core::spawn_task::SpawnTaskStatus;
use std::path::Path;

/// Format help message for /spawn command.
pub fn format_spawn_help() -> String {
    r#"Spawn Task Management

The /spawn command shows the status of background spawn tasks.

To spawn a new task, use the exec CLI with --iter or --time:
  just exec --iter 5 "implement feature X"
  just exec --time 1h "fix bugs in the codebase"

Current Spawn Tasks:"#
        .to_string()
}

/// Format task list output.
pub fn format_task_list(tasks: &[SpawnTaskMetadata]) -> String {
    if tasks.is_empty() {
        return "  No spawn tasks found.".to_string();
    }

    let mut output = String::new();
    for task in tasks {
        let status_icon = match task.status {
            SpawnTaskStatus::Running => "▶",
            SpawnTaskStatus::Completed => "✓",
            SpawnTaskStatus::Failed => "✗",
            SpawnTaskStatus::Cancelled => "○",
        };

        output.push_str(&format!(
            "\n  {} {} [{}] - {} iterations",
            status_icon, task.task_id, task.status, task.iterations_completed
        ));

        if let Some(ref query) = task.user_query {
            let truncated = if query.len() > 40 {
                format!("{}...", &query[..37])
            } else {
                query.clone()
            };
            output.push_str(&format!(" - \"{}\"", truncated));
        }

        if let Some(ref branch) = task.branch_name {
            output.push_str(&format!("\n      Branch: {branch}"));
        }
    }

    output
}

/// List all task metadata from the spawn tasks directory (synchronous).
///
/// This is a sync version for TUI use since the TUI event handlers are synchronous.
pub fn list_task_metadata_sync(codex_home: &Path) -> Result<Vec<SpawnTaskMetadata>, String> {
    let dir = codex_home.join("spawn-tasks");

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(&dir).map_err(|e| format!("Failed to read directory: {e}"))?;

    let mut result: Vec<SpawnTaskMetadata> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(metadata) = serde_json::from_str::<SpawnTaskMetadata>(&content) {
                    result.push(metadata);
                }
            }
        }
    }

    // Sort by creation time, newest first
    result.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use codex_core::loop_driver::LoopCondition;
    use codex_core::spawn_task::SpawnTaskType;
    use std::path::PathBuf;

    fn create_test_metadata(task_id: &str) -> SpawnTaskMetadata {
        SpawnTaskMetadata {
            task_id: task_id.to_string(),
            task_type: SpawnTaskType::Agent,
            status: SpawnTaskStatus::Running,
            created_at: Utc::now(),
            completed_at: None,
            cwd: PathBuf::from("/test"),
            error_message: None,
            loop_condition: Some(LoopCondition::Iters { count: 5 }),
            user_query: Some("Implement feature X".to_string()),
            iterations_completed: 2,
            iterations_failed: 0,
            workflow_path: None,
            worktree_path: None,
            branch_name: Some("spawn-task1".to_string()),
            base_branch: Some("main".to_string()),
            log_file: None,
            execution_result: None,
        }
    }

    #[test]
    fn format_empty_list() {
        let output = format_task_list(&[]);
        assert!(output.contains("No spawn tasks found"));
    }

    #[test]
    fn format_task_list_with_tasks() {
        let tasks = vec![create_test_metadata("task-1")];
        let output = format_task_list(&tasks);

        assert!(output.contains("task-1"));
        assert!(output.contains("▶")); // Running icon
        assert!(output.contains("2 iterations"));
        assert!(output.contains("Implement feature X"));
        assert!(output.contains("spawn-task1"));
    }
}
