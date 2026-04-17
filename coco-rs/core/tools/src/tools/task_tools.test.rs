//! Tests for task_tools. Focuses on the unified TaskStop behavior (B1.2)
//! which now handles background shell/agent tasks and TODO tasks through
//! the same tool entry point, matching TS `TaskStopTool.ts` + `killShellTasks.ts`.

use super::TaskCreateTool;
use super::TaskStopTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

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

#[tokio::test]
async fn test_task_stop_accepts_task_id() {
    // Create a TODO task first so we have a real ID in the store.
    let ctx = ToolUseContext::test_default();
    let create_result = TaskCreateTool
        .execute(
            json!({"subject": "test task", "description": "to be stopped"}),
            &ctx,
        )
        .await
        .unwrap();
    let task_id = create_result.data["task"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stop it via `task_id` (canonical).
    // R3: success result matches TS shape: `{message, task_id, task_type}`.
    let stop_result = TaskStopTool
        .execute(json!({"task_id": task_id.clone()}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], json!(task_id));
    assert_eq!(stop_result.data["task_type"], "todo");
    assert!(
        stop_result.data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Successfully stopped")
    );
}

#[tokio::test]
async fn test_task_stop_accepts_shell_id_alias() {
    let ctx = ToolUseContext::test_default();
    let create_result = TaskCreateTool
        .execute(
            json!({"subject": "shell alias test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let task_id = create_result.data["task"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stop via the deprecated `shell_id` alias.
    let stop_result = TaskStopTool
        .execute(json!({"shell_id": task_id.clone()}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], json!(task_id));
    assert_eq!(stop_result.data["task_type"], "todo");
}

#[tokio::test]
async fn test_task_stop_accepts_legacy_taskid_alias() {
    let ctx = ToolUseContext::test_default();
    let create_result = TaskCreateTool
        .execute(
            json!({"subject": "legacy alias test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let task_id = create_result.data["task"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stop via the legacy camelCase `taskId` alias.
    let stop_result = TaskStopTool
        .execute(json!({"taskId": task_id.clone()}), &ctx)
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], json!(task_id));
    assert_eq!(stop_result.data["task_type"], "todo");
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
        err.contains("No task found") || err.contains("not found"),
        "error should mention 'not found': {err}"
    );
}

// ---------------------------------------------------------------------------
// B4.5: TaskOutput blocking with Notify (polling-based wait)
// ---------------------------------------------------------------------------

use super::TaskOutputTool;

/// TaskOutput without block=true returns a snapshot immediately. For a
/// pending TODO task, TS emits `retrieval_status: "not_ready"` with the
/// task nested inside `task`.
#[tokio::test]
async fn test_task_output_snapshot_mode() {
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

    // TS shape: `{retrieval_status, task: {task_id, task_type, status,
    // description, output}}`.
    assert_eq!(
        result.data["retrieval_status"], "not_ready",
        "pending TODO → not_ready"
    );
    assert_eq!(result.data["task"]["task_type"], "todo");
    assert_eq!(result.data["task"]["status"], "pending");
}

/// TaskOutput accepts both `task_id` (canonical) and `taskId` (legacy).
#[tokio::test]
async fn test_task_output_accepts_legacy_taskid() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "legacy id test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    // Use the legacy camelCase form.
    let result = TaskOutputTool
        .execute(json!({"taskId": tid}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["task"]["task_id"], json!(tid));
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
    let ctx = ToolUseContext::test_default();
    let create_result = TaskCreateTool
        .execute(
            json!({"subject": "precedence test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let task_id = create_result.data["task"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Pass both task_id (valid) and shell_id (garbage). task_id should win.
    let stop_result = TaskStopTool
        .execute(
            json!({
                "task_id": task_id.clone(),
                "shell_id": "garbage-id",
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(stop_result.data["task_id"], json!(task_id));
    assert_eq!(stop_result.data["task_type"], "todo");
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

/// TaskOutput on a completed TODO task reports
/// `retrieval_status: "success"`.
#[tokio::test]
async fn test_task_output_completed_todo_is_success() {
    let ctx = ToolUseContext::test_default();
    let create = TaskCreateTool
        .execute(
            json!({"subject": "terminal test", "description": "x"}),
            &ctx,
        )
        .await
        .unwrap();
    let tid = create.data["task"]["id"].as_str().unwrap().to_string();

    // Flip it to completed.
    let _ = TaskUpdateTool
        .execute(json!({"taskId": tid.clone(), "status": "completed"}), &ctx)
        .await
        .unwrap();

    let result = TaskOutputTool
        .execute(json!({"task_id": tid}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["retrieval_status"], "success");
    assert_eq!(result.data["task"]["status"], "completed");
}
