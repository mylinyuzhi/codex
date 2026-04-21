//! Tests for task_tools.
//!
//! TS alignment contracts locked in here:
//! - TaskStop operates on the running-task registry only (TS `TaskStopTool.ts:60-91`).
//!   Plan-item IDs must error; completing/deleting plan items uses `TaskUpdate`.
//! - TaskOutput operates on the running-task registry only; unknown IDs
//!   return `{retrieval_status: "not_ready", task: null}` (TS `TaskOutputTool.tsx:53`).
//! - TaskCreate/Get/List/Update output shapes match TS byte-for-byte.

use super::TaskCreateTool;
use super::TaskStopTool;
use coco_tool::BackgroundTaskInfo;
use coco_tool::BackgroundTaskStatus;
use coco_tool::TaskHandle;
use coco_tool::TaskOutputDelta;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;
use std::sync::Arc;

/// Test double that tracks `kill_task` / `get_task_status` calls and
/// returns canned results. Exercises the TS-aligned TaskStop/TaskOutput
/// paths (which only operate on `appState.tasks`, i.e. the running-task
/// registry — represented in coco-rs by `TaskHandle`).
#[derive(Default)]
struct RecordingTaskHandle {
    known_ids: std::sync::Mutex<Vec<String>>,
    kill_calls: std::sync::Mutex<Vec<String>>,
}

impl RecordingTaskHandle {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn register(&self, task_id: &str) {
        self.known_ids.lock().unwrap().push(task_id.to_string());
    }
    fn killed(&self) -> Vec<String> {
        self.kill_calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TaskHandle for RecordingTaskHandle {
    async fn spawn_shell_task(
        &self,
        _request: coco_tool::BackgroundShellRequest,
    ) -> anyhow::Result<String> {
        unimplemented!("not used in these tests")
    }
    async fn get_task_status(&self, task_id: &str) -> anyhow::Result<BackgroundTaskInfo> {
        if self
            .known_ids
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == task_id)
        {
            Ok(BackgroundTaskInfo {
                task_id: task_id.into(),
                status: BackgroundTaskStatus::Running,
                summary: None,
                output_file: None,
                tool_use_id: None,
                elapsed_seconds: 0.0,
                notified: false,
            })
        } else {
            Err(anyhow::anyhow!("unknown background task: {task_id}"))
        }
    }
    async fn get_task_output_delta(
        &self,
        _task_id: &str,
        _from_offset: i64,
    ) -> anyhow::Result<TaskOutputDelta> {
        Ok(TaskOutputDelta {
            content: String::new(),
            new_offset: 0,
            is_complete: false,
        })
    }
    async fn kill_task(&self, task_id: &str) -> anyhow::Result<()> {
        if self
            .known_ids
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == task_id)
        {
            self.kill_calls.lock().unwrap().push(task_id.to_string());
            Ok(())
        } else {
            Err(anyhow::anyhow!("task not found: {task_id}"))
        }
    }
    async fn list_tasks(&self) -> Vec<BackgroundTaskInfo> {
        Vec::new()
    }
    async fn poll_notifications(&self) -> Vec<BackgroundTaskInfo> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// TaskStop: unified entry for shell + agent + TODO tasks
// ---------------------------------------------------------------------------

/// TaskStop must accept `task_id` (canonical), `shell_id` (deprecated), and
/// `taskId` (legacy camelCase) as equivalent parameter names. Missing all
/// three is an InvalidInput error.
#[tokio::test]
async fn test_task_stop_rejects_missing_id() {
    let ctx = ToolUseContext::test_default();
    let result = TaskStopTool.execute(json!({}), &ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("task_id"),
        "error should mention task_id: {err}"
    );
}

/// TaskStop must accept all three alias names when they resolve to a
/// registered background task. Uses a `RecordingTaskHandle` to stand in
/// for the running-task registry (TS `appState.tasks`).
#[tokio::test]
async fn test_task_stop_accepts_task_id_for_background_task() {
    let handle = RecordingTaskHandle::new();
    handle.register("bg-1");
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(handle.clone());

    let stop_result = TaskStopTool
        .execute(json!({"task_id": "bg-1"}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], "bg-1");
    assert_eq!(stop_result.data["task_type"], "background");
    assert_eq!(handle.killed(), vec!["bg-1".to_string()]);
}

#[tokio::test]
async fn test_task_stop_accepts_shell_id_alias() {
    let handle = RecordingTaskHandle::new();
    handle.register("bg-2");
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(handle.clone());

    let stop_result = TaskStopTool
        .execute(json!({"shell_id": "bg-2"}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], "bg-2");
    assert_eq!(stop_result.data["task_type"], "background");
}

#[tokio::test]
async fn test_task_stop_accepts_legacy_taskid_alias() {
    let handle = RecordingTaskHandle::new();
    handle.register("bg-3");
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(handle.clone());

    let stop_result = TaskStopTool
        .execute(json!({"taskId": "bg-3"}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], "bg-3");
    assert_eq!(stop_result.data["task_type"], "background");
}

/// TS-alignment contract: plan-item IDs (in `utils/tasks.ts` disk
/// namespace) are NOT valid for TaskStop, which only operates on
/// running tasks (`appState.tasks`). Must surface as an error so the
/// model learns to use `TaskUpdate(status=completed|deleted)` instead.
#[tokio::test]
async fn test_task_stop_rejects_plan_item_id() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(json!({"subject": "plan item", "description": "x"}), &ctx)
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    // No TaskHandle registered for this id → must error out.
    let err = TaskStopTool
        .execute(json!({"task_id": tid}), &ctx)
        .await
        .expect_err("plan-item id must not be accepted by TaskStop");
    let msg = err.to_string();
    assert!(
        msg.contains("No running task found"),
        "error must steer the model: {msg}"
    );
}

/// Unknown IDs return a structured error, not a hard panic. Because the
/// TaskHandle stage will also fail (NoOpTaskHandle → err), the final result
/// should be an error JSON payload mentioning "not found".
#[tokio::test]
async fn test_task_stop_unknown_id_returns_error() {
    // R3: unknown ID must surface as a tool error (TS throws Error),
    // not as a successful ToolResult with an `error` field. The model
    // perceives the two cases differently — errors trigger retry logic,
    // successful tool results don't.
    let ctx = ToolUseContext::test_default();
    let result = TaskStopTool
        .execute(json!({"task_id": "nonexistent-id-12345"}), &ctx)
        .await;
    assert!(result.is_err(), "unknown ID should surface as tool error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No running task found"),
        "error should name the missing running-task namespace: {err}"
    );
}

// ---------------------------------------------------------------------------
// B4.5: TaskOutput blocking with Notify (polling-based wait)
// ---------------------------------------------------------------------------

use super::TaskOutputTool;

/// TS-alignment contract: TaskOutput operates on the running-task
/// registry only (`appState.tasks`). A plan-item id is unknown from
/// that perspective and must return `{retrieval_status: "not_ready",
/// task: null}` (TS `TaskOutputTool.tsx:53`).
#[tokio::test]
async fn test_task_output_returns_null_for_plan_item_id() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "snapshot test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    let result = TaskOutputTool
        .execute(json!({"task_id": tid}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["retrieval_status"], "not_ready");
    assert!(result.data["task"].is_null());
}

/// TaskOutput accepts both `task_id` (canonical) and `taskId` (legacy).
#[tokio::test]
async fn test_task_output_accepts_legacy_taskid() {
    let handle = RecordingTaskHandle::new();
    handle.register("bg-output");
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(handle);

    let result = TaskOutputTool
        .execute(json!({"taskId": "bg-output", "block": false}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["task"]["task_id"], "bg-output");
    assert_eq!(result.data["task"]["task_type"], "background");
}

/// Unknown IDs return a structured error with `retrieval_status: "not_ready"`
/// and `task: null`, matching TS `TaskOutputTool.tsx:53`.
#[tokio::test]
async fn test_task_output_unknown_id_returns_error() {
    let ctx = ToolUseContext::test_default();
    let result = TaskOutputTool
        .execute(json!({"task_id": "nonexistent-xyz"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["retrieval_status"], "not_ready");
    assert!(result.data["task"].is_null());
    let err = result.data["error"].as_str().unwrap_or_default();
    assert!(err.contains("not found"));
}

/// Missing ID parameter → InvalidInput error.
#[tokio::test]
async fn test_task_output_rejects_missing_id() {
    let ctx = ToolUseContext::test_default();
    let result = TaskOutputTool.execute(json!({}), &ctx).await;
    assert!(result.is_err());
}

/// TS `TaskOutputTool.tsx:32` defaults `block: true`. Regression guard:
/// if we ever flip the default back to false, this test catches it.
/// Note: the TODO-task fall-through in test_default always returns
/// `blocked: false` because TODO tasks can't actually block (they're
/// synchronous). The default only matters for background tasks via
/// TaskHandle — which we can't easily exercise without an impl. We
/// instead assert the schema DEFAULT by inspecting the JSON description.
#[test]
fn test_task_output_schema_documents_block_default_true() {
    let schema = TaskOutputTool.input_schema();
    let block_prop = schema.properties.get("block").unwrap();
    let desc = block_prop["description"].as_str().unwrap();
    assert!(
        desc.contains("true (default)") || desc.contains("default true"),
        "block param description should advertise default=true, got: {desc}"
    );
}

/// Canonical parameter takes precedence over deprecated aliases when
/// multiple are provided.
#[tokio::test]
async fn test_task_stop_canonical_precedence() {
    let handle = RecordingTaskHandle::new();
    handle.register("bg-canonical");
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(handle.clone());

    // Both `task_id` (valid) and `shell_id` (garbage) — canonical must win.
    let stop_result = TaskStopTool
        .execute(
            json!({"task_id": "bg-canonical", "shell_id": "garbage-id"}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], "bg-canonical");
    assert_eq!(stop_result.data["task_type"], "background");
    assert_eq!(handle.killed(), vec!["bg-canonical".to_string()]);
}

// ---------------------------------------------------------------------------
// R4-T7: TodoWriteTool TS-alignment
// ---------------------------------------------------------------------------
//
// TS `tools/TodoWriteTool/TodoWriteTool.ts` uses replace-all semantics —
// the model sends the complete list on every call, prior contents are
// replaced, and the response returns `{oldTodos, newTodos,
// verificationNudgeNeeded}`. Each `TodoItem` must have `content`,
// `status`, and `activeForm` (min-length 1 each, no `id`). These tests
// lock in the schema + output shape so regressions are caught early.

use super::TodoWriteTool;

/// Schema must match TS `utils/todo/types.ts::TodoItemSchema`:
///   - items have `content`, `status`, `activeForm` (all required)
///   - NO `id` field
#[test]
fn test_todo_write_schema_matches_ts() {
    let schema = TodoWriteTool.input_schema();
    let todos_prop = schema.properties.get("todos").unwrap();
    let items = &todos_prop["items"];
    let required = items["required"].as_array().unwrap();

    // Must require exactly content, status, activeForm.
    let required_set: std::collections::HashSet<_> = required
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert!(required_set.contains("content"));
    assert!(required_set.contains("status"));
    assert!(required_set.contains("activeForm"));
    assert!(!required_set.contains("id"), "TS TodoItem has no id field");

    // Status enum matches TS.
    let status_enum = items["properties"]["status"]["enum"].as_array().unwrap();
    let enum_set: std::collections::HashSet<_> =
        status_enum.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        enum_set,
        ["pending", "in_progress", "completed"]
            .into_iter()
            .collect()
    );
}

/// Round-trip: write a todo list, verify the output has TS-shaped
/// `{oldTodos, newTodos, verificationNudgeNeeded}`.
#[tokio::test]
async fn test_todo_write_output_shape_matches_ts() {
    let ctx = ToolUseContext::test_default();

    // Clear any leftover state from parallel tests.
    let _ = TodoWriteTool
        .execute(json!({"todos": []}), &ctx)
        .await
        .unwrap();

    let result = TodoWriteTool
        .execute(
            json!({
                "todos": [
                    {
                        "content": "write the tests",
                        "status": "pending",
                        "activeForm": "Writing the tests"
                    }
                ]
            }),
            &ctx,
        )
        .await
        .unwrap();

    // TS output keys.
    assert!(result.data["oldTodos"].is_array(), "oldTodos missing");
    assert!(result.data["newTodos"].is_array(), "newTodos missing");
    assert!(
        result.data["verificationNudgeNeeded"].is_boolean(),
        "verificationNudgeNeeded missing"
    );

    let new_todos = result.data["newTodos"].as_array().unwrap();
    assert_eq!(new_todos.len(), 1);
    assert_eq!(new_todos[0]["content"], "write the tests");
    assert_eq!(new_todos[0]["activeForm"], "Writing the tests");
    assert!(
        new_todos[0].get("id").is_none(),
        "coco-rs must not emit an id field (TS doesn't)"
    );
}

/// Missing `activeForm` is rejected (TS `min(1)` constraint).
#[tokio::test]
async fn test_todo_write_rejects_missing_active_form() {
    let ctx = ToolUseContext::test_default();
    let result = TodoWriteTool
        .execute(
            json!({
                "todos": [
                    {"content": "x", "status": "pending"}
                ]
            }),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "missing activeForm must error");
}

/// Empty `content` is rejected.
#[tokio::test]
async fn test_todo_write_rejects_empty_content() {
    let ctx = ToolUseContext::test_default();
    let result = TodoWriteTool
        .execute(
            json!({
                "todos": [
                    {"content": "", "status": "pending", "activeForm": "Doing"}
                ]
            }),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "empty content must error");
}

/// Invalid status is rejected.
#[tokio::test]
async fn test_todo_write_rejects_bad_status() {
    let ctx = ToolUseContext::test_default();
    let result = TodoWriteTool
        .execute(
            json!({
                "todos": [
                    {"content": "x", "status": "cancelled", "activeForm": "Doing"}
                ]
            }),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "status=cancelled must error");
}

// ---------------------------------------------------------------------------
// R5-T10: TS-shaped output schemas for TaskCreate/Get/List/Update/Output
// ---------------------------------------------------------------------------
//
// TS task tools return minimal wrapped JSON (`{task: {...}}`,
// `{tasks: [...]}`) so internal fields like `output`, `active_form`,
// `metadata` never leak to the model. These tests lock in each TS shape.

use super::TaskGetTool;
use super::TaskListTool;
use super::TaskUpdateTool;

/// TS `TaskCreateTool.ts:36-43` — output is `{task: {id, subject}}` only.
/// No description, metadata, owner, etc.
#[tokio::test]
async fn test_task_create_output_shape_ts() {
    let ctx = ToolUseContext::test_default();
    let result = TaskCreateTool
        .execute(
            json!({
                "subject": "write docs",
                "description": "update the CLAUDE.md",
                "activeForm": "Writing docs",
                "metadata": {"priority": "high"}
            }),
            &ctx,
        )
        .await
        .unwrap();

    // Exactly {task: {id, subject}} — no other keys leak.
    let task = result.data["task"].as_object().unwrap();
    assert_eq!(task.len(), 2, "task object should have exactly 2 keys");
    assert!(task["id"].is_string());
    assert_eq!(task["subject"], "write docs");
    // Fields that MUST NOT leak:
    assert!(
        task.get("description").is_none(),
        "description must not leak"
    );
    assert!(task.get("metadata").is_none(), "metadata must not leak");
    assert!(
        task.get("active_form").is_none() && task.get("activeForm").is_none(),
        "active_form must not leak"
    );
    assert!(task.get("output").is_none(), "output must not leak");
}

/// TS `TaskGetTool.ts:20-32` — wrapped `{task: {id, subject, description,
/// status, blocks, blockedBy} | null}`.
#[tokio::test]
async fn test_task_get_output_shape_ts() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "test get", "description": "test description"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    let result = TaskGetTool
        .execute(json!({"taskId": tid.clone()}), &ctx)
        .await
        .unwrap();

    // Wrapped in `task`.
    let task = result.data["task"].as_object().unwrap();
    assert_eq!(task["id"], json!(tid));
    assert_eq!(task["subject"], "test get");
    assert_eq!(task["description"], "test description");
    assert_eq!(task["status"], "pending");
    assert!(task["blocks"].is_array());
    assert!(task["blockedBy"].is_array());
    // owner/metadata/active_form/output must NOT leak.
    assert!(task.get("owner").is_none());
    assert!(task.get("metadata").is_none());
    assert!(task.get("output").is_none());
    assert!(task.get("active_form").is_none());
}

/// TS `TaskGetTool.ts` — unknown id returns `{task: null}`, not an error.
#[tokio::test]
async fn test_task_get_unknown_returns_null() {
    let ctx = ToolUseContext::test_default();
    let result = TaskGetTool
        .execute(json!({"taskId": "no-such-id"}), &ctx)
        .await
        .unwrap();
    assert!(result.data["task"].is_null());
}

/// TS `TaskListTool.ts:16-28` — `{tasks: [...]}` with 5 fields per entry
/// (id, subject, status, owner?, blockedBy).
#[tokio::test]
async fn test_task_list_output_shape_ts() {
    let ctx = ToolUseContext::test_default();
    // Use unique subjects so we can find our tasks in a potentially
    // non-empty store.
    let unique = format!(
        "t10-list-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _ = TaskCreateTool
        .execute(
            json!({"subject": &unique, "description": "check list shape"}),
            &ctx,
        )
        .await
        .unwrap();

    let result = TaskListTool.execute(json!({}), &ctx).await.unwrap();

    let tasks = result.data["tasks"].as_array().unwrap();
    // Find our task by subject.
    let ours = tasks
        .iter()
        .find(|t| t["subject"].as_str() == Some(&unique))
        .expect("our task should be in the list");
    assert!(ours["id"].is_string());
    assert_eq!(ours["status"], "pending");
    assert!(ours["blockedBy"].is_array());
    // description/metadata/output/active_form must NOT leak.
    assert!(ours.get("description").is_none());
    assert!(ours.get("metadata").is_none());
    assert!(ours.get("output").is_none());
}

/// TS `TaskListTool.ts:68-69` filters tasks whose metadata has `_internal`.
#[tokio::test]
async fn test_task_list_filters_internal_tasks() {
    let ctx = ToolUseContext::test_default();
    // Create a visible task.
    let unique_visible = format!(
        "visible-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _ = TaskCreateTool
        .execute(
            json!({"subject": &unique_visible, "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    // Create an internal task with _internal metadata.
    let unique_internal = format!(
        "internal-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _ = TaskCreateTool
        .execute(
            json!({
                "subject": &unique_internal,
                "description": "y",
                "metadata": {"_internal": true}
            }),
            &ctx,
        )
        .await
        .unwrap();

    let result = TaskListTool.execute(json!({}), &ctx).await.unwrap();
    let tasks = result.data["tasks"].as_array().unwrap();
    assert!(
        tasks
            .iter()
            .any(|t| t["subject"].as_str() == Some(&unique_visible)),
        "visible task should appear"
    );
    assert!(
        !tasks
            .iter()
            .any(|t| t["subject"].as_str() == Some(&unique_internal)),
        "_internal task must be filtered out"
    );
}

/// TS `TaskUpdateTool.ts:69-83` — output is
/// `{success, taskId, updatedFields, statusChange?}`.
#[tokio::test]
async fn test_task_update_output_shape_ts() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "test update", "description": "start"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    let result = TaskUpdateTool
        .execute(
            json!({
                "taskId": tid.clone(),
                "status": "in_progress",
                "description": "new description"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["success"], true);
    assert_eq!(result.data["taskId"], json!(tid));
    let updated: std::collections::HashSet<_> = result.data["updatedFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(updated.contains("status"));
    assert!(updated.contains("description"));
    // statusChange emitted when status changed.
    assert_eq!(result.data["statusChange"]["from"], "pending");
    assert_eq!(result.data["statusChange"]["to"], "in_progress");
}

/// Unknown task id → `{success: false, taskId, updatedFields: [], error}`.
#[tokio::test]
async fn test_task_update_unknown_id_shape() {
    let ctx = ToolUseContext::test_default();
    let result = TaskUpdateTool
        .execute(
            json!({"taskId": "no-such-task", "status": "completed"}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(result.data["success"], false);
    assert_eq!(result.data["taskId"], "no-such-task");
    assert!(result.data["error"].as_str().unwrap().contains("not found"));
}

// ---------------------------------------------------------------------------
// Phase C: app_state_patch — tools surface snapshots to AppState for the TUI.
// ---------------------------------------------------------------------------

/// TaskCreate returns an `app_state_patch` that fills `plan_tasks`,
/// sets `expanded_view = Tasks`. Matches TS `TaskCreateTool.ts:116-119`.
#[tokio::test]
async fn test_task_create_emits_snapshot_and_auto_expand() {
    let ctx = ToolUseContext::test_default();
    let result = TaskCreateTool
        .execute(json!({"subject": "panel item", "description": "x"}), &ctx)
        .await
        .unwrap();

    let patch = result.app_state_patch.expect("patch must be emitted");
    let mut state = coco_types::ToolAppState::default();
    patch(&mut state);
    assert_eq!(state.plan_tasks.len(), 1);
    assert_eq!(state.plan_tasks[0].subject, "panel item");
    assert_eq!(state.expanded_view, coco_types::ExpandedView::Tasks);
    assert!(!state.verification_nudge_pending);
}

/// TaskUpdate on completion with all-done gate sets
/// `verification_nudge_pending = true`.
#[tokio::test]
async fn test_task_update_sets_verification_nudge_in_patch() {
    let mut ctx = ToolUseContext::test_default();
    ctx.agent_id = None; // main thread
    let mut ids = Vec::new();
    for i in 0..3 {
        let r = TaskCreateTool
            .execute(
                json!({"subject": format!("step {i}"), "description": ""}),
                &ctx,
            )
            .await
            .unwrap();
        ids.push(r.data["task"]["id"].as_str().unwrap().to_string());
    }
    for id in &ids[..2] {
        let _ = TaskUpdateTool
            .execute(json!({"taskId": id, "status": "completed"}), &ctx)
            .await
            .unwrap();
    }
    // Final completion — patch must flip the nudge flag.
    let result = TaskUpdateTool
        .execute(json!({"taskId": ids[2], "status": "completed"}), &ctx)
        .await
        .unwrap();
    let patch = result.app_state_patch.expect("patch must be emitted");
    let mut state = coco_types::ToolAppState::default();
    patch(&mut state);
    assert!(state.verification_nudge_pending);
    assert_eq!(state.expanded_view, coco_types::ExpandedView::Tasks);
}

/// TodoWrite emits a patch keyed by agent_id (or session fallback).
#[tokio::test]
async fn test_todo_write_emits_snapshot_keyed_by_agent() {
    use coco_types::AgentId;
    let mut ctx = ToolUseContext::test_default();
    ctx.agent_id = Some(AgentId::new("subagent-7"));

    let result = TodoWriteTool
        .execute(
            json!({"todos": [
                {"content": "item", "status": "pending", "activeForm": "Doing it"}
            ]}),
            &ctx,
        )
        .await
        .unwrap();
    let patch = result.app_state_patch.expect("patch must be emitted");
    let mut state = coco_types::ToolAppState::default();
    patch(&mut state);
    let list = state
        .todos_by_agent
        .get("subagent-7")
        .expect("todos for subagent-7");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].content, "item");
    assert_eq!(list[0].active_form, "Doing it");
}

/// Plan-item completion is observed via `TaskGet`, not `TaskOutput`.
/// This locks in the TS-aligned separation: TaskOutput doesn't know
/// about plan items.
#[tokio::test]
async fn test_task_get_surfaces_completed_plan_item() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "terminal test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    let _ = TaskUpdateTool
        .execute(json!({"taskId": tid.clone(), "status": "completed"}), &ctx)
        .await
        .unwrap();

    let got = TaskGetTool
        .execute(json!({"taskId": tid}), &ctx)
        .await
        .unwrap();
    assert_eq!(got.data["task"]["status"], "completed");
}

// ---------------------------------------------------------------------------
// Phase 5: TS parity behaviors — deleted status, auto-owner, blockedBy filter,
// verification nudge, mailbox notify on owner change.
// ---------------------------------------------------------------------------

use coco_tool::InboxMessage;
use coco_tool::MailboxEnvelope;
use coco_tool::MailboxHandle;

/// `status=deleted` permanently removes the task. TS `TaskUpdateTool.ts:213-226`.
#[tokio::test]
async fn test_task_update_delete_status_removes_task() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(json!({"subject": "to delete", "description": "x"}), &ctx)
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    let result = TaskUpdateTool
        .execute(json!({"taskId": tid.clone(), "status": "deleted"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["success"], true);
    assert_eq!(result.data["statusChange"]["to"], "deleted");

    // Subsequent get must return null.
    let got = TaskGetTool
        .execute(json!({"taskId": tid}), &ctx)
        .await
        .unwrap();
    assert!(got.data["task"].is_null(), "deleted task should vanish");
}

/// TaskList filters resolved blockers from `blockedBy` so the model only
/// sees currently-active dependencies. TS `TaskListTool.ts:72-83`.
#[tokio::test]
async fn test_task_list_filters_resolved_blockers_from_blocked_by() {
    let ctx = ToolUseContext::test_default();
    let a = TaskCreateTool
        .execute(json!({"subject": "a", "description": ""}), &ctx)
        .await
        .unwrap();
    let b = TaskCreateTool
        .execute(json!({"subject": "b", "description": ""}), &ctx)
        .await
        .unwrap();
    let a_id = a.data["task"]["id"].as_str().unwrap().to_string();
    let b_id = b.data["task"]["id"].as_str().unwrap().to_string();

    // Set: a blocks b → b.blockedBy = [a].
    let _ = TaskUpdateTool
        .execute(
            json!({"taskId": b_id.clone(), "addBlockedBy": [a_id.clone()]}),
            &ctx,
        )
        .await
        .unwrap();

    // Before resolving a, b's blockedBy contains a.
    let list = TaskListTool.execute(json!({}), &ctx).await.unwrap();
    let b_entry = list.data["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == json!(b_id))
        .unwrap()
        .clone();
    assert_eq!(
        b_entry["blockedBy"].as_array().unwrap().len(),
        1,
        "b should be blocked by a before a completes"
    );

    // After a completes, b.blockedBy should be filtered out.
    let _ = TaskUpdateTool
        .execute(json!({"taskId": a_id, "status": "completed"}), &ctx)
        .await
        .unwrap();
    let list = TaskListTool.execute(json!({}), &ctx).await.unwrap();
    let b_entry = list.data["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == json!(b_id))
        .unwrap()
        .clone();
    assert!(
        b_entry["blockedBy"].as_array().unwrap().is_empty(),
        "b.blockedBy should be empty once a is completed"
    );
}

/// Verification nudge fires when main thread closes out 3+ tasks and
/// none match /verif/i. TS `TaskUpdateTool.ts:334-349`.
#[tokio::test]
async fn test_task_update_verification_nudge_main_thread_all_done() {
    let mut ctx = ToolUseContext::test_default();
    ctx.agent_id = None; // main thread
    let mut ids = Vec::new();
    for i in 0..3 {
        let created = TaskCreateTool
            .execute(
                json!({"subject": format!("step {i}"), "description": ""}),
                &ctx,
            )
            .await
            .unwrap();
        ids.push(created.data["task"]["id"].as_str().unwrap().to_string());
    }

    // Complete the first two with no nudge yet.
    for id in ids.iter().take(2) {
        let res = TaskUpdateTool
            .execute(json!({"taskId": id, "status": "completed"}), &ctx)
            .await
            .unwrap();
        assert_eq!(res.data["verificationNudgeNeeded"], false);
    }

    // Final completion → all done, none match verify → nudge.
    let res = TaskUpdateTool
        .execute(json!({"taskId": ids[2], "status": "completed"}), &ctx)
        .await
        .unwrap();
    assert_eq!(res.data["verificationNudgeNeeded"], true);
}

/// Nudge skipped when a verification task exists.
#[tokio::test]
async fn test_task_update_verification_nudge_skipped_when_verify_task_exists() {
    let mut ctx = ToolUseContext::test_default();
    ctx.agent_id = None;
    let mut ids = Vec::new();
    for subject in ["impl", "test", "Verify output"] {
        let created = TaskCreateTool
            .execute(json!({"subject": subject, "description": ""}), &ctx)
            .await
            .unwrap();
        ids.push(created.data["task"]["id"].as_str().unwrap().to_string());
    }
    for id in &ids[..2] {
        let _ = TaskUpdateTool
            .execute(json!({"taskId": id, "status": "completed"}), &ctx)
            .await
            .unwrap();
    }
    let res = TaskUpdateTool
        .execute(json!({"taskId": ids[2], "status": "completed"}), &ctx)
        .await
        .unwrap();
    assert_eq!(
        res.data["verificationNudgeNeeded"], false,
        "verify task present → no nudge"
    );
}

/// Nudge gate: subagent context (agent_id set) never receives the nudge.
#[tokio::test]
async fn test_task_update_verification_nudge_skipped_in_subagent() {
    use coco_types::AgentId;
    let mut ctx = ToolUseContext::test_default();
    ctx.agent_id = Some(AgentId::new("subagent-1"));
    let mut ids = Vec::new();
    for i in 0..3 {
        let created = TaskCreateTool
            .execute(
                json!({"subject": format!("step {i}"), "description": ""}),
                &ctx,
            )
            .await
            .unwrap();
        ids.push(created.data["task"]["id"].as_str().unwrap().to_string());
    }
    for id in &ids[..2] {
        let _ = TaskUpdateTool
            .execute(json!({"taskId": id, "status": "completed"}), &ctx)
            .await
            .unwrap();
    }
    let res = TaskUpdateTool
        .execute(json!({"taskId": ids[2], "status": "completed"}), &ctx)
        .await
        .unwrap();
    assert_eq!(
        res.data["verificationNudgeNeeded"], false,
        "subagents never receive the nudge"
    );
}

// Recording mailbox for test assertions.
struct RecordingMailbox {
    written: std::sync::Mutex<Vec<(String, String, MailboxEnvelope)>>,
}

impl RecordingMailbox {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            written: std::sync::Mutex::new(Vec::new()),
        })
    }
    fn calls(&self) -> Vec<(String, String, MailboxEnvelope)> {
        self.written.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl MailboxHandle for RecordingMailbox {
    async fn write_to_mailbox(
        &self,
        recipient: &str,
        team_name: &str,
        message: MailboxEnvelope,
    ) -> anyhow::Result<()> {
        self.written
            .lock()
            .unwrap()
            .push((recipient.to_string(), team_name.to_string(), message));
        Ok(())
    }
    async fn read_unread(
        &self,
        _agent_name: &str,
        _team_name: &str,
    ) -> anyhow::Result<Vec<InboxMessage>> {
        Ok(Vec::new())
    }
    async fn mark_read(
        &self,
        _agent_name: &str,
        _team_name: &str,
        _index: usize,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Setting an owner in a teammate context writes a `task_assignment`
/// envelope to the new owner's mailbox. TS `TaskUpdateTool.ts:277-298`.
#[tokio::test]
async fn test_task_update_owner_change_writes_mailbox() {
    let mailbox = RecordingMailbox::new();
    let mut ctx = ToolUseContext::test_default();
    ctx.is_teammate = true;
    ctx.agent_name = Some("leader".into());
    ctx.team_name = Some("alpha-team".into());
    ctx.mailbox = mailbox.clone();

    let created = TaskCreateTool
        .execute(
            json!({"subject": "fix bug", "description": "find root cause"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = created.data["task"]["id"].as_str().unwrap().to_string();

    let _ = TaskUpdateTool
        .execute(json!({"taskId": tid.clone(), "owner": "bob"}), &ctx)
        .await
        .unwrap();

    let calls = mailbox.calls();
    assert_eq!(calls.len(), 1, "exactly one mailbox write expected");
    assert_eq!(calls[0].0, "bob", "recipient = new owner");
    assert_eq!(calls[0].1, "alpha-team");
    let payload: serde_json::Value = serde_json::from_str(&calls[0].2.text).unwrap();
    assert_eq!(payload["type"], "task_assignment");
    assert_eq!(payload["taskId"], json!(tid));
    assert_eq!(payload["assignedBy"], "leader");
}

/// Auto-owner: when a teammate marks a task in_progress without an
/// explicit owner and the task is unclaimed, the teammate auto-claims
/// it. TS `TaskUpdateTool.ts:188-199`.
#[tokio::test]
async fn test_task_update_auto_owner_on_in_progress() {
    let mailbox = RecordingMailbox::new();
    let mut ctx = ToolUseContext::test_default();
    ctx.is_teammate = true;
    ctx.agent_name = Some("alice".into());
    ctx.team_name = Some("alpha-team".into());
    ctx.mailbox = mailbox.clone();

    let created = TaskCreateTool
        .execute(json!({"subject": "claim me", "description": "x"}), &ctx)
        .await
        .unwrap();
    let tid = created.data["task"]["id"].as_str().unwrap().to_string();

    let res = TaskUpdateTool
        .execute(
            json!({"taskId": tid.clone(), "status": "in_progress"}),
            &ctx,
        )
        .await
        .unwrap();
    let has_owner = res.data["updatedFields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("owner"));
    assert!(
        has_owner,
        "auto-owner should be in updatedFields: {:?}",
        res.data["updatedFields"]
    );

    let got = TaskGetTool
        .execute(json!({"taskId": tid}), &ctx)
        .await
        .unwrap();
    let _ = got; // owner isn't projected into TaskGet output; validate via mailbox write
    let calls = mailbox.calls();
    assert_eq!(calls.len(), 1, "auto-owner must also trigger mailbox");
    assert_eq!(calls[0].0, "alice");
}

/// Auto-owner does NOT kick in for non-teammate contexts.
#[tokio::test]
async fn test_task_update_auto_owner_skipped_outside_swarm() {
    let mut ctx = ToolUseContext::test_default();
    ctx.is_teammate = false;
    ctx.agent_name = Some("alice".into());

    let created = TaskCreateTool
        .execute(json!({"subject": "x", "description": ""}), &ctx)
        .await
        .unwrap();
    let tid = created.data["task"]["id"].as_str().unwrap().to_string();

    let res = TaskUpdateTool
        .execute(json!({"taskId": tid, "status": "in_progress"}), &ctx)
        .await
        .unwrap();
    let has_owner = res.data["updatedFields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("owner"));
    assert!(!has_owner, "non-teammate must not auto-claim");
}
