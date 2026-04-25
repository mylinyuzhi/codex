//! Implementations of the seven task/todo tools, all on top of the
//! shared `TaskListHandle` + `TodoListHandle` injected through
//! `ToolUseContext`.
//!
//! **TS alignment**: see `tools/Task{Create,Get,List,Update,Stop,Output}Tool/`
//! plus `tools/TodoWriteTool/`. Output projections are the exact TS shapes
//! (JSON envelopes like a `task` wrapper or a `tasks` array) so the model
//! sees the same payloads as in TS.

use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::MailboxEnvelope;
use coco_tool_runtime::TaskListHandleRef;
use coco_tool_runtime::TaskListStatus;
use coco_tool_runtime::TaskRecord;
use coco_tool_runtime::TaskRecordUpdate;
use coco_tool_runtime::TodoListHandleRef;
use coco_tool_runtime::TodoRecord;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::AppStatePatch;
use coco_types::ExpandedView;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── Output projections (TS-shaped) ────────────────────────────────────

/// Shape a freshly-created task for TaskCreate's `data`.
/// TS `TaskCreateTool.ts:36-43` — `{task: {id, subject}}`.
fn project_create(task: &TaskRecord) -> Value {
    serde_json::json!({
        "task": { "id": task.id, "subject": task.subject }
    })
}

/// TS `TaskGetTool.ts:20-32` — `{task: {id, subject, description, status,
/// blocks, blockedBy} | null}`.
fn project_get(task: Option<&TaskRecord>) -> Value {
    match task {
        Some(t) => serde_json::json!({
            "task": {
                "id": t.id,
                "subject": t.subject,
                "description": t.description,
                "status": t.status.as_str(),
                "blocks": t.blocks,
                "blockedBy": t.blocked_by,
            }
        }),
        None => serde_json::json!({ "task": null }),
    }
}

/// One TaskList entry: 4-5 fields (id, subject, status, blockedBy,
/// owner?). Completed tasks resolve blockers out of blockedBy (TS
/// `TaskListTool.ts:72-83`).
fn project_list_entry(
    task: &TaskRecord,
    resolved_ids: &std::collections::HashSet<String>,
) -> Value {
    let filtered_blocked_by: Vec<&String> = task
        .blocked_by
        .iter()
        .filter(|id| !resolved_ids.contains(*id))
        .collect();
    let mut obj = serde_json::json!({
        "id": task.id,
        "subject": task.subject,
        "status": task.status.as_str(),
        "blockedBy": filtered_blocked_by,
    });
    if let Some(owner) = &task.owner {
        obj["owner"] = Value::String(owner.clone());
    }
    obj
}

fn is_internal_task(task: &TaskRecord) -> bool {
    task.metadata
        .as_ref()
        .is_some_and(|m| m.contains_key("_internal"))
}

fn project_update(
    task_id: &str,
    updated_fields: Vec<&'static str>,
    status_change: Option<(String, String)>,
    verification_nudge: bool,
) -> Value {
    let mut out = serde_json::json!({
        "success": true,
        "taskId": task_id,
        "updatedFields": updated_fields,
        "verificationNudgeNeeded": verification_nudge,
    });
    if let Some((from, to)) = status_change {
        out["statusChange"] = serde_json::json!({ "from": from, "to": to });
    }
    out
}

fn project_update_error(task_id: &str, error: &str) -> Value {
    serde_json::json!({
        "success": false,
        "taskId": task_id,
        "updatedFields": [],
        "error": error,
    })
}

#[derive(Debug, Clone, Copy)]
enum RetrievalStatus {
    Success,
    Timeout,
    NotReady,
}

impl RetrievalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Timeout => "timeout",
            Self::NotReady => "not_ready",
        }
    }
}

fn project_output_background(
    task_id: &str,
    task_status: &str,
    output: String,
    exit_code: Option<i32>,
    status: RetrievalStatus,
) -> Value {
    let mut task_obj = serde_json::json!({
        "task_id": task_id,
        "task_type": "background",
        "status": task_status,
        "description": "",
        "output": output,
    });
    if let Some(code) = exit_code {
        task_obj["exitCode"] = Value::Number(serde_json::Number::from(code));
    }
    serde_json::json!({
        "retrieval_status": status.as_str(),
        "task": task_obj,
    })
}

// ── app_state patch helpers ───────────────────────────────────────────

/// Snapshot the current plan-task list (filtered to model-visible
/// entries) and return a patch that:
/// 1. Stores the snapshot in `ToolAppState.plan_tasks`
/// 2. Sets `expanded_view = Tasks` (matches TS auto-expand)
/// 3. Updates `verification_nudge_pending`
///
/// The snapshot is computed *now* and moved into the closure. The
/// executor applies the patch post-execute under a single write lock.
async fn build_task_list_patch(
    task_list: &TaskListHandleRef,
    verification_nudge: bool,
) -> AppStatePatch {
    let mut visible = task_list.list_tasks().await.unwrap_or_default();
    visible.retain(|t| {
        !t.metadata
            .as_ref()
            .is_some_and(|m| m.contains_key("_internal"))
    });
    Box::new(move |state: &mut coco_types::ToolAppState| {
        state.plan_tasks = visible;
        state.expanded_view = ExpandedView::Tasks;
        if verification_nudge {
            state.verification_nudge_pending = true;
        }
    })
}

/// After TodoWrite, snapshot the store for `key` and patch AppState.
/// TodoWrite doesn't auto-expand by itself in TS, but we still update
/// the shared snapshot so the TUI can render V1 lists.
async fn build_todo_patch(
    todo_list: &TodoListHandleRef,
    key: String,
    verification_nudge: bool,
) -> AppStatePatch {
    let items = todo_list.read(&key).await;
    Box::new(move |state: &mut coco_types::ToolAppState| {
        if items.is_empty() {
            state.todos_by_agent.remove(&key);
        } else {
            state.todos_by_agent.insert(key, items);
        }
        if verification_nudge {
            state.verification_nudge_pending = true;
        }
    })
}

/// Key under which a TodoWrite list is stored in `TodoListHandle`.
///
/// TS `TodoWriteTool.ts:67`: `const todoKey = context.agentId ?? getSessionId()`.
/// In coco-rs the session id is passed to `ToolUseContext` as
/// `session_id_for_history` at bootstrap; we use that as the fallback.
/// As a last resort (tests without either), use a stable literal so
/// list-local operations remain round-trippable.
fn todo_key(ctx: &ToolUseContext) -> String {
    ctx.agent_id
        .as_ref()
        .map(ToString::to_string)
        .or_else(|| ctx.session_id_for_history.clone())
        .unwrap_or_else(|| "main-session".to_string())
}

// ── TaskCreateTool ────────────────────────────────────────────────────

pub struct TaskCreateTool;

#[async_trait::async_trait]
impl Tool for TaskCreateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskCreate)
    }
    fn name(&self) -> &str {
        ToolName::TaskCreate.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Create a new task with a subject and description.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "subject".into(),
            serde_json::json!({"type": "string", "description": "Task subject/title"}),
        );
        p.insert(
            "description".into(),
            serde_json::json!({"type": "string", "description": "Detailed task description"}),
        );
        p.insert(
            "activeForm".into(),
            serde_json::json!({"type": "string", "description": "Present continuous form shown in spinner when in_progress (e.g., 'Running tests')"}),
        );
        p.insert(
            "metadata".into(),
            serde_json::json!({"type": "object", "description": "Arbitrary metadata to attach to the task"}),
        );
        ToolInputSchema { properties: p }
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let subject = input
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled task")
            .to_string();
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let active_form = input
            .get("activeForm")
            .and_then(|v| v.as_str())
            .map(String::from);
        let metadata = input
            .get("metadata")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

        let task = ctx
            .task_list
            .create_task(subject, description, active_form, metadata)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("task_list.create_task failed: {e}"),
                source: None,
            })?;

        // TS `TaskCreateTool.ts:116-119` — auto-expand the task panel.
        let patch = build_task_list_patch(&ctx.task_list, false).await;
        Ok(ToolResult {
            data: project_create(&task),
            new_messages: vec![],
            app_state_patch: Some(patch),
        })
    }
}

// ── TaskGetTool ───────────────────────────────────────────────────────

pub struct TaskGetTool;

#[async_trait::async_trait]
impl Tool for TaskGetTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskGet)
    }
    fn name(&self) -> &str {
        ToolName::TaskGet.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Get the status and details of a task by its ID.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "taskId".into(),
            serde_json::json!({"type": "string", "description": "The task ID to look up"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input
            .get("taskId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "taskId parameter is required".into(),
                error_code: None,
            });
        }
        let task =
            ctx.task_list
                .get_task(&task_id)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("task_list.get_task failed: {e}"),
                    source: None,
                })?;
        Ok(ToolResult {
            data: project_get(task.as_ref()),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskListTool ──────────────────────────────────────────────────────

pub struct TaskListTool;

#[async_trait::async_trait]
impl Tool for TaskListTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskList)
    }
    fn name(&self) -> &str {
        ToolName::TaskList.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "List all tasks and their current status.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let all = ctx
            .task_list
            .list_tasks()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("task_list.list_tasks failed: {e}"),
                source: None,
            })?;

        // TS `TaskListTool.ts:73-76` — any completed task id is removed
        // from other tasks' `blockedBy` so the model only sees unresolved
        // blockers.
        let resolved_ids: std::collections::HashSet<String> = all
            .iter()
            .filter(|t| t.status == TaskListStatus::Completed)
            .map(|t| t.id.clone())
            .collect();

        let tasks: Vec<Value> = all
            .iter()
            .filter(|t| !is_internal_task(t))
            .map(|t| project_list_entry(t, &resolved_ids))
            .collect();

        Ok(ToolResult {
            data: serde_json::json!({ "tasks": tasks }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskUpdateTool ────────────────────────────────────────────────────

pub struct TaskUpdateTool;

#[async_trait::async_trait]
impl Tool for TaskUpdateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskUpdate)
    }
    fn name(&self) -> &str {
        ToolName::TaskUpdate.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Update a task's status, dependencies, or metadata.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "taskId".into(),
            serde_json::json!({"type": "string", "description": "The task ID to update"}),
        );
        p.insert(
            "status".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["pending", "in_progress", "completed", "deleted"],
                "description": "New status — 'deleted' permanently removes the task"
            }),
        );
        p.insert(
            "subject".into(),
            serde_json::json!({"type": "string", "description": "New subject for the task"}),
        );
        p.insert(
            "description".into(),
            serde_json::json!({"type": "string", "description": "New description"}),
        );
        p.insert(
            "activeForm".into(),
            serde_json::json!({"type": "string", "description": "Present continuous form for spinner"}),
        );
        p.insert(
            "owner".into(),
            serde_json::json!({"type": "string", "description": "New owner (agent name)"}),
        );
        p.insert(
            "addBlocks".into(),
            serde_json::json!({
                "type": "array", "items": {"type": "string"},
                "description": "Task IDs that cannot start until this one completes"
            }),
        );
        p.insert(
            "addBlockedBy".into(),
            serde_json::json!({
                "type": "array", "items": {"type": "string"},
                "description": "Task IDs that must complete before this one can start"
            }),
        );
        p.insert(
            "metadata".into(),
            serde_json::json!({
                "type": "object",
                "description": "Metadata keys to merge (set key to null to delete)"
            }),
        );
        ToolInputSchema { properties: p }
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input
            .get("taskId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "taskId parameter is required".into(),
                error_code: None,
            });
        }

        // Fetch existing — TS `TaskUpdateTool.ts:146-156` returns
        // `{success: false, error}` when the task is missing rather than
        // erroring out, so the model can handle it gracefully.
        let existing = match ctx.task_list.get_task(&task_id).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                return Ok(ToolResult {
                    data: project_update_error(&task_id, "Task not found"),
                    new_messages: vec![],
                    app_state_patch: None,
                });
            }
            Err(e) => {
                return Err(ToolError::ExecutionFailed {
                    message: format!("task_list.get_task failed: {e}"),
                    source: None,
                });
            }
        };

        let mut updated_fields: Vec<&'static str> = Vec::new();

        // ── Handle `status=deleted` — delete the task and return early.
        // TS `TaskUpdateTool.ts:213-226`.
        if let Some("deleted") = input.get("status").and_then(|v| v.as_str()) {
            let deleted = ctx.task_list.delete_task(&task_id).await.map_err(|e| {
                ToolError::ExecutionFailed {
                    message: format!("task_list.delete_task failed: {e}"),
                    source: None,
                }
            })?;
            let status_change = if deleted {
                Some((existing.status.as_str().to_string(), "deleted".into()))
            } else {
                None
            };
            let data = if deleted {
                let mut out = project_update(&task_id, vec!["deleted"], status_change, false);
                out["success"] = Value::Bool(true);
                out
            } else {
                project_update_error(&task_id, "Failed to delete task")
            };
            let patch = build_task_list_patch(&ctx.task_list, false).await;
            return Ok(ToolResult {
                data,
                new_messages: vec![],
                app_state_patch: Some(patch),
            });
        }

        // ── Regular updates ──────────────────────────────────────────
        let mut update = TaskRecordUpdate::default();
        let mut status_change: Option<(String, String)> = None;
        let mut newly_completed = false;

        if let Some(s) = input.get("subject").and_then(|v| v.as_str())
            && s != existing.subject
        {
            update.subject = Some(s.to_string());
            updated_fields.push("subject");
        }
        if let Some(d) = input.get("description").and_then(|v| v.as_str())
            && d != existing.description
        {
            update.description = Some(d.to_string());
            updated_fields.push("description");
        }
        if let Some(af) = input.get("activeForm").and_then(|v| v.as_str())
            && Some(af) != existing.active_form.as_deref()
        {
            update.active_form = Some(af.to_string());
            updated_fields.push("activeForm");
        }

        let requested_owner = input
            .get("owner")
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some(o) = &requested_owner
            && existing.owner.as_deref() != Some(o.as_str())
        {
            update.owner = Some(o.clone());
            updated_fields.push("owner");
        }

        // Auto-owner assignment: when a teammate sets status=in_progress
        // without an explicit owner and the task is unclaimed, auto-
        // assign. TS `TaskUpdateTool.ts:188-199`.
        if let Some("in_progress") = input.get("status").and_then(|v| v.as_str())
            && requested_owner.is_none()
            && existing.owner.is_none()
            && ctx.is_teammate
            && let Some(name) = ctx.agent_name.as_deref()
            && !name.is_empty()
        {
            update.owner = Some(name.to_string());
            if !updated_fields.contains(&"owner") {
                updated_fields.push("owner");
            }
        }

        // Status transition — reject "deleted" (handled above) and
        // unknown enum values.
        if let Some(status_str) = input.get("status").and_then(|v| v.as_str()) {
            let new_status = match status_str {
                "pending" => TaskListStatus::Pending,
                "in_progress" => TaskListStatus::InProgress,
                "completed" => TaskListStatus::Completed,
                other => {
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "Invalid status '{other}'. Must be pending, in_progress, completed, or deleted"
                        ),
                        error_code: None,
                    });
                }
            };
            if new_status != existing.status {
                // Fire pre-hook via task-list store: the store runs
                // `HookEventType::TaskCompleted` on transition to
                // Completed (see `task_list.rs`). We could also run
                // local pre-checks here, but to match TS `TaskUpdateTool.ts:232-265`
                // the hook fires from inside `update_task`.
                update.status = Some(new_status);
                status_change = Some((existing.status.as_str().into(), new_status.as_str().into()));
                updated_fields.push("status");
                if new_status == TaskListStatus::Completed {
                    newly_completed = true;
                }
            }
        }

        // Metadata merge (null deletions handled inside the store).
        if let Some(meta) = input.get("metadata").and_then(|v| v.as_object()) {
            let merge: HashMap<String, Value> =
                meta.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            if !merge.is_empty() {
                update.metadata_merge = Some(merge);
                updated_fields.push("metadata");
            }
        }

        // Persist the update.
        if update_has_changes(&update) {
            ctx.task_list
                .update_task(&task_id, update.clone())
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("task_list.update_task failed: {e}"),
                    source: None,
                })?;
        }

        // Add blocks / blockedBy edges (these go through block_task).
        if let Some(add_blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            let mut any_added = false;
            for id_v in add_blocks {
                if let Some(id) = id_v.as_str() {
                    let added = ctx
                        .task_list
                        .block_task(&task_id, id)
                        .await
                        .unwrap_or(false);
                    if added {
                        any_added = true;
                    }
                }
            }
            if any_added {
                updated_fields.push("blocks");
            }
        }
        if let Some(add_blocked) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            let mut any_added = false;
            for id_v in add_blocked {
                if let Some(id) = id_v.as_str() {
                    let added = ctx
                        .task_list
                        .block_task(id, &task_id)
                        .await
                        .unwrap_or(false);
                    if added {
                        any_added = true;
                    }
                }
            }
            if any_added {
                updated_fields.push("blockedBy");
            }
        }

        // Mailbox-notify the new owner (swarm only). TS `TaskUpdateTool.ts:277-298`.
        if let Some(new_owner) = update.owner.as_deref()
            && ctx.is_teammate
            && let Some(team_name) = ctx.team_name.as_deref()
        {
            let sender = ctx
                .agent_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("team-lead");
            let body = serde_json::json!({
                "type": "task_assignment",
                "taskId": task_id,
                "subject": existing.subject,
                "description": existing.description,
                "assignedBy": sender,
                "timestamp": now_iso(),
            })
            .to_string();
            let envelope = MailboxEnvelope {
                text: body,
                from: sender.to_string(),
                timestamp: now_iso(),
            };
            // Best effort — failures in mailbox routing shouldn't block
            // the core task update.
            let _ = ctx
                .mailbox
                .write_to_mailbox(new_owner, team_name, envelope)
                .await;
        }

        // Verification nudge — TS `TaskUpdateTool.ts:334-349`.
        let is_main_thread = ctx.agent_id.is_none();
        let verification_nudge = ctx
            .task_list
            .should_nudge_verification(newly_completed, is_main_thread)
            .await;

        // TS `TaskUpdateTool.ts:140-143` — auto-expand on update.
        let patch = build_task_list_patch(&ctx.task_list, verification_nudge).await;
        Ok(ToolResult {
            data: project_update(&task_id, updated_fields, status_change, verification_nudge),
            new_messages: vec![],
            app_state_patch: Some(patch),
        })
    }
}

fn update_has_changes(u: &TaskRecordUpdate) -> bool {
    u.subject.is_some()
        || u.description.is_some()
        || u.active_form.is_some()
        || u.owner.is_some()
        || u.status.is_some()
        || u.metadata_merge.is_some()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── TaskStopTool ──────────────────────────────────────────────────────

pub struct TaskStopTool;

#[async_trait::async_trait]
impl Tool for TaskStopTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskStop)
    }
    fn name(&self) -> &str {
        ToolName::TaskStop.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Stop a running background task by its ID. For TODO-style plan \
         items (created via TaskCreate), use TaskUpdate with status \
         'completed' or 'deleted' instead — plan items are not tracked \
         in the running-task registry."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "task_id".into(),
            serde_json::json!({
                "type": "string",
                "description": "The task ID to stop. Accepts IDs returned by TaskCreate, \
                                Agent (subagent spawn), or Bash (run_in_background=true)."
            }),
        );
        p.insert(
            "shell_id".into(),
            serde_json::json!({
                "type": "string",
                "description": "Deprecated alias for task_id (KillShell compatibility)."
            }),
        );
        p.insert(
            "taskId".into(),
            serde_json::json!({
                "type": "string",
                "description": "Legacy camelCase alias for task_id."
            }),
        );
        ToolInputSchema { properties: p }
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = first_non_empty(&[
            input.get("task_id"),
            input.get("shell_id"),
            input.get("taskId"),
        ])
        .unwrap_or_default();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "Missing required parameter: task_id".into(),
                error_code: Some("1".into()),
            });
        }

        // TS `TaskStopTool.ts:60-91` `validateInput`: the task must live
        // in the running-task registry (`appState.tasks[id]`). Plan items
        // live in a disjoint namespace (`utils/tasks.ts` on disk) and
        // must be terminated via `TaskUpdate(status=completed|deleted)`
        // — **not** this tool.
        let Some(handle) = ctx.task_handle.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "No running task found with ID: {task_id} (no task runtime configured)"
                ),
                source: None,
            });
        };
        match handle.kill_task(&task_id).await {
            Ok(()) => Ok(ToolResult {
                data: serde_json::json!({
                    "message": format!("Successfully stopped task: {task_id}"),
                    "task_id": task_id,
                    "task_type": "background",
                }),
                new_messages: vec![],
                app_state_patch: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("No running task found with ID: {task_id} ({e})"),
                source: None,
            }),
        }
    }
}

fn first_non_empty(candidates: &[Option<&Value>]) -> Option<String> {
    for v in candidates {
        if let Some(s) = v.and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

// ── TaskOutputTool ────────────────────────────────────────────────────

pub struct TaskOutputTool;

#[async_trait::async_trait]
impl Tool for TaskOutputTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskOutput)
    }
    fn name(&self) -> &str {
        ToolName::TaskOutput.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Read the output of a completed or running task. With block=true (default), \
         waits for the task to complete (or reach `timeout` milliseconds) before \
         returning. Works for both TODO plan items and background shell/agent tasks."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "task_id".into(),
            serde_json::json!({
                "type": "string",
                "description": "The task ID to get output for (canonical name)"
            }),
        );
        p.insert(
            "taskId".into(),
            serde_json::json!({
                "type": "string",
                "description": "Legacy camelCase alias for task_id"
            }),
        );
        p.insert(
            "block".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "When true (default), wait for the task to complete before returning. \
                                Set to false for an immediate snapshot."
            }),
        );
        p.insert(
            "timeout".into(),
            serde_json::json!({
                "type": "number",
                "description": "Blocking timeout in milliseconds (default 30000). Polls every 100ms."
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id =
            first_non_empty(&[input.get("task_id"), input.get("taskId")]).unwrap_or_default();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "task_id (or taskId) parameter is required".into(),
                error_code: None,
            });
        }
        let block = input
            .get("block")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(30_000);

        // Stage 1: background task namespace.
        if let Some(handle) = ctx.task_handle.as_ref()
            && let Ok(initial) = handle.get_task_status(&task_id).await
        {
            use coco_tool_runtime::BackgroundTaskStatus;
            let info = if block {
                wait_for_task_completion(handle.as_ref(), &task_id, initial, timeout_ms).await
            } else {
                initial
            };
            let output = handle
                .get_task_output_delta(&task_id, 0)
                .await
                .map(|d| d.content)
                .unwrap_or_default();
            let retrieval = match info.status {
                BackgroundTaskStatus::Completed
                | BackgroundTaskStatus::Failed
                | BackgroundTaskStatus::Killed => RetrievalStatus::Success,
                _ if block => RetrievalStatus::Timeout,
                _ => RetrievalStatus::NotReady,
            };
            let status_str = format!("{:?}", info.status).to_lowercase();
            return Ok(ToolResult {
                data: project_output_background(
                    &info.task_id,
                    &status_str,
                    output,
                    None,
                    retrieval,
                ),
                new_messages: vec![],
                app_state_patch: None,
            });
        }

        // TS `TaskOutputTool.tsx:53` — unknown IDs fall through to
        // `{retrieval_status: 'not_ready', task: null}`. Plan items live
        // in a disjoint namespace and are inspected via `TaskGet`; we do
        // not read them here.
        Ok(ToolResult {
            data: serde_json::json!({
                "retrieval_status": RetrievalStatus::NotReady.as_str(),
                "task": null,
                "error": format!("Task '{task_id}' not found"),
            }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

async fn wait_for_task_completion(
    handle: &dyn coco_tool_runtime::TaskHandle,
    task_id: &str,
    initial: coco_tool_runtime::BackgroundTaskInfo,
    timeout_ms: u64,
) -> coco_tool_runtime::BackgroundTaskInfo {
    use coco_tool_runtime::BackgroundTaskStatus;
    if matches!(
        initial.status,
        BackgroundTaskStatus::Completed
            | BackgroundTaskStatus::Failed
            | BackgroundTaskStatus::Killed
    ) {
        return initial;
    }
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let interval = std::time::Duration::from_millis(100);
    let mut last = initial;
    while start.elapsed() < timeout {
        tokio::time::sleep(interval).await;
        match handle.get_task_status(task_id).await {
            Ok(info) => {
                let terminal = matches!(
                    info.status,
                    BackgroundTaskStatus::Completed
                        | BackgroundTaskStatus::Failed
                        | BackgroundTaskStatus::Killed
                );
                last = info;
                if terminal {
                    return last;
                }
            }
            Err(_) => return last,
        }
    }
    last
}

// ── TodoWriteTool ─────────────────────────────────────────────────────

pub struct TodoWriteTool;

#[async_trait::async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TodoWrite)
    }
    fn name(&self) -> &str {
        ToolName::TodoWrite.as_str()
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Write or update the in-conversation TODO list. Pass the full list each call; \
         the prior list is replaced. Each item requires content, status, and activeForm."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "todos".into(),
            serde_json::json!({
                "type": "array",
                "description": "The updated todo list",
                "items": {
                    "type": "object",
                    "required": ["content", "status", "activeForm"],
                    "properties": {
                        "content": {"type": "string", "minLength": 1, "description": "Task description"},
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"],
                            "description": "Task status"
                        },
                        "activeForm": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Verb phrase shown while the task is in progress"
                        }
                    }
                }
            }),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let todos_value = input.get("todos").cloned().unwrap_or(Value::Array(vec![]));
        let incoming: Vec<TodoRecord> =
            serde_json::from_value(todos_value).map_err(|e| ToolError::InvalidInput {
                message: format!("Invalid todos format: {e}"),
                error_code: None,
            })?;

        for (i, item) in incoming.iter().enumerate() {
            if item.content.is_empty() {
                return Err(ToolError::InvalidInput {
                    message: format!("todos[{i}].content cannot be empty"),
                    error_code: None,
                });
            }
            if item.active_form.is_empty() {
                return Err(ToolError::InvalidInput {
                    message: format!("todos[{i}].activeForm cannot be empty"),
                    error_code: None,
                });
            }
            if !matches!(
                item.status.as_str(),
                "pending" | "in_progress" | "completed"
            ) {
                return Err(ToolError::InvalidInput {
                    message: format!(
                        "todos[{i}].status must be pending, in_progress, or completed"
                    ),
                    error_code: None,
                });
            }
        }

        let key = todo_key(ctx);
        let old_todos = ctx.todo_list.read(&key).await;

        // `allDone → clear` (TS line 69-70).
        let all_done = !incoming.is_empty() && incoming.iter().all(|t| t.status == "completed");
        let to_store = if all_done {
            Vec::new()
        } else {
            incoming.clone()
        };
        ctx.todo_list.write(&key, to_store).await;

        // Verification nudge — main-thread only, all done, ≥3 items,
        // no verify matches. Main-thread here = `ctx.agent_id.is_none()`.
        let is_main_thread = ctx.agent_id.is_none();
        let verification_nudge = if is_main_thread && all_done {
            let contents: Vec<&str> = incoming.iter().map(|i| i.content.as_str()).collect();
            coco_tool_runtime::check_verification_nudge(&contents)
        } else {
            false
        };

        let old_json = serde_json::to_value(&old_todos).unwrap_or_default();
        let new_json = serde_json::to_value(&incoming).unwrap_or_default();
        let patch = build_todo_patch(&ctx.todo_list, key, verification_nudge).await;
        Ok(ToolResult {
            data: serde_json::json!({
                "oldTodos": old_json,
                "newTodos": new_json,
                "verificationNudgeNeeded": verification_nudge,
            }),
            new_messages: vec![],
            app_state_patch: Some(patch),
        })
    }
}

#[cfg(test)]
#[path = "task_tools.test.rs"]
mod tests;
