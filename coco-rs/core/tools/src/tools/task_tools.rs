//! Implementations of the seven task/todo tools, all on top of the
//! shared `TaskListHandle` + `TodoListHandle` injected through
//! `ToolUseContext`.
//!
//! **TS alignment**: see `tools/Task{Create,Get,List,Update,Stop,Output}Tool/`
//! plus `tools/TodoWriteTool/`. Output projections are the exact TS shapes
//! (JSON envelopes like a `task` wrapper or a `tasks` array) so the model
//! sees the same payloads as in TS.

use coco_messages::ToolResult;
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
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::AppStatePatch;
use coco_types::ExpandedView;
use coco_types::Feature;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
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
    completed_nudge: bool,
) -> Value {
    let mut out = serde_json::json!({
        "success": true,
        "taskId": task_id,
        "updatedFields": updated_fields,
        "verificationNudgeNeeded": verification_nudge,
        "completedNudgeNeeded": completed_nudge,
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

/// Typed input for [`TaskCreateTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskCreateInput {
    /// Task subject/title
    #[serde(default)]
    pub subject: Option<String>,
    /// Detailed task description
    #[serde(default)]
    pub description: Option<String>,
    /// Present continuous form shown in spinner when in_progress
    /// (e.g., 'Running tests')
    #[serde(default, rename = "activeForm")]
    pub active_form: Option<String>,
    /// Arbitrary metadata to attach to the task
    #[serde(default)]
    pub metadata: Option<HashMap<String, Value>>,
}

pub struct TaskCreateTool;

#[async_trait::async_trait]
impl Tool for TaskCreateTool {
    type Input = TaskCreateInput;
    coco_tool_runtime::impl_runtime_schema!(TaskCreateInput);
    /// Output is a TS-shaped `{task: {...}}` envelope built by
    /// `project_create`. Kept as `Value` because the projection helper
    /// is shared across the 7 task tools and they share field-shape
    /// invariants the renderer reads positionally.
    type Output = Value;

    fn to_auto_classifier_input(&self, input: &TaskCreateInput) -> Option<String> {
        Some(input.subject.clone().unwrap_or_default())
    }

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskCreate)
    }
    fn name(&self) -> &str {
        ToolName::TaskCreate.as_str()
    }
    fn description(&self, _input: &TaskCreateInput, _options: &DescriptionOptions) -> String {
        "Create a new task with a subject and description.".into()
    }

    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::TaskV2)
    }

    fn is_concurrency_safe(&self, _input: &TaskCreateInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("create a persistent task in the plan-item store")
    }

    /// Render the create envelope as `Task #{id} created successfully: {subject}`.
    /// TS parity: `TaskCreateTool.ts:130-135::mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let task = data.get("task");
        let id = task
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        let subject = task
            .and_then(|t| t.get("subject"))
            .and_then(Value::as_str)
            .unwrap_or("");
        vec![ToolResultContentPart::Text {
            text: format!("Task #{id} created successfully: {subject}"),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TaskCreateInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let subject = input
            .subject
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled task".to_string());
        let description = input.description.unwrap_or_default();
        let active_form = input.active_form;
        let metadata = input.metadata;

        let task = ctx
            .task_list
            .create_task(subject.clone(), description.clone(), active_form, metadata)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("task_list.create_task failed: {e}"),
                display_data: None,
                source: None,
            })?;

        // TS `TaskCreateTool.ts:122-152` — fire TaskCreated hooks AFTER
        // the task is persisted. A blocking hook rolls the task back so
        // the model sees the failure and the store stays consistent.
        if let Some(handle) = ctx.hook_handle.as_ref() {
            let outcome = handle
                .run_task_created(
                    &task.id,
                    &subject,
                    if description.is_empty() {
                        None
                    } else {
                        Some(description.as_str())
                    },
                    /*teammate_name*/ None,
                    /*team_name*/ None,
                )
                .await;
            if let Some(reason) = outcome.blocking_reason {
                let _ = ctx.task_list.delete_task(&task.id).await;
                return Err(ToolError::ExecutionFailed {
                    message: format!("TaskCreated hook feedback:\n{reason}"),
                    display_data: None,
                    source: None,
                });
            }
        }

        // TS `TaskCreateTool.ts:116-119` — auto-expand the task panel.
        let patch = build_task_list_patch(&ctx.task_list, false).await;
        Ok(ToolResult {
            data: project_create(&task),
            new_messages: vec![],
            app_state_patch: Some(patch),
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

// ── TaskGetTool ───────────────────────────────────────────────────────

/// Typed input for [`TaskGetTool`]. Wire key is `taskId` (camelCase)
/// for TS parity.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskGetInput {
    /// The task ID to look up
    #[serde(default, rename = "taskId")]
    pub task_id: String,
}

pub struct TaskGetTool;

#[async_trait::async_trait]
impl Tool for TaskGetTool {
    type Input = TaskGetInput;
    coco_tool_runtime::impl_runtime_schema!(TaskGetInput);
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskGet)
    }
    fn name(&self) -> &str {
        ToolName::TaskGet.as_str()
    }
    fn description(&self, _input: &TaskGetInput, _options: &DescriptionOptions) -> String {
        "Get the status and details of a task by its ID.".into()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::TaskV2)
    }
    fn is_read_only(&self, _input: &TaskGetInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _input: &TaskGetInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("fetch a single task record by id")
    }

    /// Render the task envelope as a multi-line text block, or
    /// "Task not found" when `task` is null. TS parity:
    /// `TaskGetTool.ts:99-128::mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let task = data.get("task");
        let text = match task {
            Some(t) if !t.is_null() => format_task_full(t),
            _ => "Task not found".to_string(),
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TaskGetInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input.task_id;
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
                    display_data: None,
                    source: None,
                })?;
        Ok(ToolResult {
            data: project_get(task.as_ref()),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

/// Render a task object into the multi-line TaskGet text block. TS
/// parity: `TaskGetTool.ts:107-122` — uses `#` prefix on every id, and
/// always includes the Description line (TS doesn't conditionally
/// suppress on empty).
fn format_task_full(task: &Value) -> String {
    let id = task.get("id").and_then(Value::as_str).unwrap_or("?");
    let subject = task.get("subject").and_then(Value::as_str).unwrap_or("");
    let status = task.get("status").and_then(Value::as_str).unwrap_or("?");
    let description = task
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut out = format!("Task #{id}: {subject}\nStatus: {status}\nDescription: {description}");
    if let Some(arr) = task.get("blockedBy").and_then(Value::as_array)
        && !arr.is_empty()
    {
        let ids: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| format!("#{s}")))
            .collect();
        out.push_str(&format!("\nBlocked by: {}", ids.join(", ")));
    }
    if let Some(arr) = task.get("blocks").and_then(Value::as_array)
        && !arr.is_empty()
    {
        let ids: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| format!("#{s}")))
            .collect();
        out.push_str(&format!("\nBlocks: {}", ids.join(", ")));
    }
    out
}

// ── TaskListTool ──────────────────────────────────────────────────────

/// Typed input for [`TaskListTool`] — no parameters.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskListInput {}

pub struct TaskListTool;

#[async_trait::async_trait]
impl Tool for TaskListTool {
    type Input = TaskListInput;
    coco_tool_runtime::impl_runtime_schema!(TaskListInput);
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskList)
    }
    fn name(&self) -> &str {
        ToolName::TaskList.as_str()
    }
    fn description(&self, _input: &TaskListInput, _options: &DescriptionOptions) -> String {
        "List all tasks and their current status.".into()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::TaskV2)
    }
    fn is_read_only(&self, _input: &TaskListInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _input: &TaskListInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("list persistent tasks filtered by status or owner")
    }

    /// Render `{tasks: [...]}` as `#{id} [{status}] {subject}{owner}{blocked}`
    /// per line. Empty list collapses to "No tasks found". TS parity:
    /// `TaskListTool.ts:91-115::mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let tasks = data.get("tasks").and_then(Value::as_array);
        let text = match tasks {
            Some(arr) if arr.is_empty() => "No tasks found".to_string(),
            Some(arr) => arr
                .iter()
                .map(|t| {
                    let id = t.get("id").and_then(Value::as_str).unwrap_or("?");
                    let subject = t.get("subject").and_then(Value::as_str).unwrap_or("");
                    let status = t.get("status").and_then(Value::as_str).unwrap_or("?");
                    let owner = t
                        .get("owner")
                        .and_then(Value::as_str)
                        .map(|o| format!(" ({o})"))
                        .unwrap_or_default();
                    let blocked = t
                        .get("blockedBy")
                        .and_then(Value::as_array)
                        .filter(|a| !a.is_empty())
                        .map(|arr| {
                            let ids: Vec<String> = arr
                                .iter()
                                .filter_map(|v| v.as_str().map(|s| format!("#{s}")))
                                .collect();
                            format!(" [blocked by {}]", ids.join(", "))
                        })
                        .unwrap_or_default();
                    format!("#{id} [{status}] {subject}{owner}{blocked}")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => serde_json::to_string(data).unwrap_or_default(),
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        _input: TaskListInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let all = ctx
            .task_list
            .list_tasks()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("task_list.list_tasks failed: {e}"),
                display_data: None,
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
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

// ── TaskUpdateTool ────────────────────────────────────────────────────

/// Typed input for [`TaskUpdateTool`]. Wire keys preserve TS camelCase
/// (`taskId`, `activeForm`, `addBlocks`, `addBlockedBy`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskUpdateInput {
    /// The task ID to update
    #[serde(default, rename = "taskId")]
    pub task_id: String,
    /// New status — 'deleted' permanently removes the task. Stored as
    /// `Option<String>` (not an enum) because the legal-value check
    /// happens inside `execute` and produces a TS-shaped error for
    /// unknown values instead of a generic serde error.
    #[serde(default)]
    pub status: Option<String>,
    /// New subject for the task
    #[serde(default)]
    pub subject: Option<String>,
    /// New description
    #[serde(default)]
    pub description: Option<String>,
    /// Present continuous form for spinner
    #[serde(default, rename = "activeForm")]
    pub active_form: Option<String>,
    /// New owner (agent name)
    #[serde(default)]
    pub owner: Option<String>,
    /// Task IDs that cannot start until this one completes
    #[serde(default, rename = "addBlocks")]
    pub add_blocks: Option<Vec<String>>,
    /// Task IDs that must complete before this one can start
    #[serde(default, rename = "addBlockedBy")]
    pub add_blocked_by: Option<Vec<String>>,
    /// Metadata keys to merge (set key to null to delete)
    #[serde(default)]
    pub metadata: Option<HashMap<String, Value>>,
}

pub struct TaskUpdateTool;

#[async_trait::async_trait]
impl Tool for TaskUpdateTool {
    type Input = TaskUpdateInput;
    coco_tool_runtime::impl_runtime_schema!(TaskUpdateInput);
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskUpdate)
    }
    fn name(&self) -> &str {
        ToolName::TaskUpdate.as_str()
    }
    fn description(&self, _input: &TaskUpdateInput, _options: &DescriptionOptions) -> String {
        "Update a task's status, dependencies, or metadata.".into()
    }

    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::TaskV2)
    }

    fn is_concurrency_safe(&self, _input: &TaskUpdateInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("update task status fields owner or notes")
    }

    /// Render the update envelope. TS parity:
    /// `TaskUpdateTool.ts:364-405::mapToolResultToToolResultBlockParam`.
    /// Error: surface the `error` field directly (or `Task #{id} not
    /// found` fallback). Success: `Updated task #{id} {fields...}` with
    /// optional teammate-completion + verification nudges.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let success = data
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let task_id = data.get("taskId").and_then(Value::as_str).unwrap_or("?");
        let text = if success {
            let updated_fields: Vec<&str> = data
                .get("updatedFields")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let mut out = format!("Updated task #{task_id} {}", updated_fields.join(", "));
            // TS `TaskUpdateTool.ts:386-394`: teammate-completion
            // nudge fires before the verification nudge when both
            // would apply.
            if data
                .get("completedNudgeNeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                out.push_str("\n\nTask completed. Call TaskList now to find your next available task or see if your work unblocked others.");
            }
            if data
                .get("verificationNudgeNeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                out.push_str("\n\nNOTE: You just closed out 3+ tasks and none of them was a verification step. Before writing your final summary, spawn the verification agent (subagent_type=\"verification-agent\"). You cannot self-assign PARTIAL by listing caveats in your summary — only the verifier issues a verdict.");
            }
            out
        } else {
            data.get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Task #{task_id} not found"))
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TaskUpdateInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input.task_id.clone();
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
                    permission_updates: Vec::new(),
                    display_data: None,
                });
            }
            Err(e) => {
                return Err(ToolError::ExecutionFailed {
                    message: format!("task_list.get_task failed: {e}"),
                    display_data: None,
                    source: None,
                });
            }
        };

        let mut updated_fields: Vec<&'static str> = Vec::new();

        // ── Handle `status=deleted` — delete the task and return early.
        // TS `TaskUpdateTool.ts:213-226`.
        if input.status.as_deref() == Some("deleted") {
            let deleted = ctx.task_list.delete_task(&task_id).await.map_err(|e| {
                ToolError::ExecutionFailed {
                    message: format!("task_list.delete_task failed: {e}"),
                    display_data: None,
                    source: None,
                }
            })?;
            let status_change = if deleted {
                Some((existing.status.as_str().to_string(), "deleted".into()))
            } else {
                None
            };
            let data = if deleted {
                let mut out =
                    project_update(&task_id, vec!["deleted"], status_change, false, false);
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
                permission_updates: Vec::new(),
                display_data: None,
            });
        }

        // ── Regular updates ──────────────────────────────────────────
        let mut update = TaskRecordUpdate::default();
        let mut status_change: Option<(String, String)> = None;
        let mut newly_completed = false;

        if let Some(s) = input.subject.as_deref()
            && s != existing.subject
        {
            update.subject = Some(s.to_string());
            updated_fields.push("subject");
        }
        if let Some(d) = input.description.as_deref()
            && d != existing.description
        {
            update.description = Some(d.to_string());
            updated_fields.push("description");
        }
        if let Some(af) = input.active_form.as_deref()
            && Some(af) != existing.active_form.as_deref()
        {
            update.active_form = Some(af.to_string());
            updated_fields.push("activeForm");
        }

        let requested_owner = input.owner.clone();
        if let Some(o) = &requested_owner
            && existing.owner.as_deref() != Some(o.as_str())
        {
            update.owner = Some(o.clone());
            updated_fields.push("owner");
        }

        // Auto-owner assignment: when a teammate sets status=in_progress
        // without an explicit owner and the task is unclaimed, auto-
        // assign. TS `TaskUpdateTool.ts:188-199`.
        if input.status.as_deref() == Some("in_progress")
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
        if let Some(status_str) = input.status.as_deref() {
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
        if let Some(merge) = input.metadata
            && !merge.is_empty()
        {
            update.metadata_merge = Some(merge);
            updated_fields.push("metadata");
        }

        // TaskCompleted hook fires BEFORE the status flip is persisted
        // so a blocking hook leaves the task in its current state. TS:
        // `executeTaskCompletedHooks` (`utils/hooks.ts:3789`) runs from
        // `TaskUpdateTool.ts:232-265` before the store write.
        if newly_completed && let Some(handle) = ctx.hook_handle.as_ref() {
            let outcome = handle
                .run_task_completed(
                    &task_id,
                    &existing.subject,
                    if existing.description.is_empty() {
                        None
                    } else {
                        Some(existing.description.as_str())
                    },
                    /*teammate_name*/ None,
                    /*team_name*/ None,
                )
                .await;
            if let Some(reason) = outcome.blocking_reason {
                return Err(ToolError::ExecutionFailed {
                    message: format!("TaskCompleted hook feedback:\n{reason}"),
                    display_data: None,
                    source: None,
                });
            }
        }

        // Persist the update.
        if update_has_changes(&update) {
            ctx.task_list
                .update_task(&task_id, update.clone())
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("task_list.update_task failed: {e}"),
                    display_data: None,
                    source: None,
                })?;
        }

        // Add blocks / blockedBy edges (these go through block_task).
        if let Some(add_blocks) = input.add_blocks.as_deref() {
            let mut any_added = false;
            for id in add_blocks {
                let added = ctx
                    .task_list
                    .block_task(&task_id, id)
                    .await
                    .unwrap_or(false);
                if added {
                    any_added = true;
                }
            }
            if any_added {
                updated_fields.push("blocks");
            }
        }
        if let Some(add_blocked) = input.add_blocked_by.as_deref() {
            let mut any_added = false;
            for id in add_blocked {
                let added = ctx
                    .task_list
                    .block_task(id, &task_id)
                    .await
                    .unwrap_or(false);
                if added {
                    any_added = true;
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

        // Teammate completion nudge — TS `TaskUpdateTool.ts:386-394`.
        // Fires when a swarm teammate (in-process or otherwise)
        // transitions a task to completed; primes the next TaskList
        // call so the agent picks up unblocked downstream work.
        let completed_nudge = newly_completed && ctx.is_teammate;

        // TS `TaskUpdateTool.ts:140-143` — auto-expand on update.
        let patch = build_task_list_patch(&ctx.task_list, verification_nudge).await;
        Ok(ToolResult {
            data: project_update(
                &task_id,
                updated_fields,
                status_change,
                verification_nudge,
                completed_nudge,
            ),
            new_messages: vec![],
            app_state_patch: Some(patch),
            permission_updates: Vec::new(),
            display_data: None,
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

/// Typed input for [`TaskStopTool`]. The model can supply any of three
/// aliased keys — `task_id` (canonical), `shell_id` (KillShell
/// compatibility), or `taskId` (legacy camelCase). Schemars derives a
/// schema that advertises ALL three as separate fields so the model
/// sees the same surface as the hand-written schema; `serde(alias)`
/// would only accept multiple wire names but emit one in the schema.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskStopInput {
    /// The task ID to stop. Accepts IDs returned by TaskCreate,
    /// Agent (subagent spawn), or Bash (run_in_background=true).
    #[serde(default)]
    pub task_id: Option<String>,
    /// Deprecated alias for task_id (KillShell compatibility).
    #[serde(default)]
    pub shell_id: Option<String>,
    /// Legacy camelCase alias for task_id.
    #[serde(default, rename = "taskId")]
    pub task_id_camel: Option<String>,
}

pub struct TaskStopTool;

#[async_trait::async_trait]
impl Tool for TaskStopTool {
    type Input = TaskStopInput;
    coco_tool_runtime::impl_runtime_schema!(TaskStopInput);
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskStop)
    }
    fn name(&self) -> &str {
        ToolName::TaskStop.as_str()
    }
    fn description(&self, _input: &TaskStopInput, _options: &DescriptionOptions) -> String {
        "Stop a running background task by its ID. For TODO-style plan \
         items (created via TaskCreate), use TaskUpdate with status \
         'completed' or 'deleted' instead — plan items are not tracked \
         in the running-task registry."
            .into()
    }

    fn is_concurrency_safe(&self, _input: &TaskStopInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("stop a running background task or shell")
    }

    // TS `TaskStopTool.ts:98-103` emits `jsonStringify(output)` —
    // i.e. the entire `{message, task_id, task_type}` envelope as
    // JSON. This matches the trait's default `render_for_model` impl
    // exactly, so no override is needed.

    async fn execute(
        &self,
        input: TaskStopInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = [
            input.task_id.as_deref(),
            input.shell_id.as_deref(),
            input.task_id_camel.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .map(String::from)
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
                display_data: None,
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
                permission_updates: Vec::new(),
                display_data: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("No running task found with ID: {task_id} ({e})"),
                display_data: None,
                source: None,
            }),
        }
    }
}

// ── TaskOutputTool ────────────────────────────────────────────────────

/// Typed input for [`TaskOutputTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TaskOutputInput {
    /// The task ID to get output for (canonical name)
    #[serde(default)]
    pub task_id: Option<String>,
    /// Legacy camelCase alias for task_id
    #[serde(default, rename = "taskId")]
    pub task_id_camel: Option<String>,
    /// When true (default), wait for the task to complete before
    /// returning. Set to false for an immediate snapshot.
    #[serde(default = "default_true")]
    pub block: bool,
    /// Blocking timeout in milliseconds (default 30000).
    #[serde(default)]
    pub timeout: Option<u64>,
}

fn default_true() -> bool {
    true
}

pub struct TaskOutputTool;

#[async_trait::async_trait]
impl Tool for TaskOutputTool {
    type Input = TaskOutputInput;
    coco_tool_runtime::impl_runtime_schema!(TaskOutputInput);
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskOutput)
    }
    fn name(&self) -> &str {
        ToolName::TaskOutput.as_str()
    }
    fn description(&self, _input: &TaskOutputInput, _options: &DescriptionOptions) -> String {
        "Retrieves output from a running or completed background task — a shell \
         launched with `run_in_background`, an async agent spawn, or a remote \
         session. With block=true (default), waits for the task to complete \
         (or reach `timeout` milliseconds) before returning. For plan items \
         created via TaskCreate, use TaskGet — they live in a separate namespace."
            .into()
    }
    fn is_read_only(&self, _input: &TaskOutputInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _input: &TaskOutputInput) -> bool {
        true
    }

    /// Render the retrieval envelope as TS-shaped XML tags. TS parity:
    /// `TaskOutputTool.tsx:283-308::mapToolResultToToolResultBlockParam`.
    /// Format:
    /// ```text
    /// <retrieval_status>STATUS</retrieval_status>
    ///
    /// <task_id>ID</task_id>
    ///
    /// <task_type>TYPE</task_type>
    ///
    /// <status>TASK_STATUS</status>
    ///
    /// <exit_code>N</exit_code>      # only when present
    ///
    /// <output>
    /// CAPTURED_OUTPUT
    /// </output>                     # only when output is non-empty
    ///
    /// <error>...</error>            # only when present
    /// ```
    /// Pieces are joined with `\n\n`. Missing fields are skipped (TS
    /// `if (data.task.exitCode !== undefined && data.task.exitCode !== null)`).
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let status = data
            .get("retrieval_status")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut parts: Vec<String> = vec![format!("<retrieval_status>{status}</retrieval_status>")];
        if let Some(task) = data.get("task")
            && !task.is_null()
        {
            let task_id = task.get("task_id").and_then(Value::as_str).unwrap_or("");
            let task_type = task.get("task_type").and_then(Value::as_str).unwrap_or("");
            let task_status = task.get("status").and_then(Value::as_str).unwrap_or("");
            parts.push(format!("<task_id>{task_id}</task_id>"));
            parts.push(format!("<task_type>{task_type}</task_type>"));
            parts.push(format!("<status>{task_status}</status>"));
            if let Some(code) = task.get("exitCode").and_then(Value::as_i64) {
                parts.push(format!("<exit_code>{code}</exit_code>"));
            }
            if let Some(output) = task.get("output").and_then(Value::as_str)
                && !output.trim().is_empty()
            {
                parts.push(format!("<output>\n{}\n</output>", output.trim_end()));
            }
            if let Some(err) = task.get("error").and_then(Value::as_str) {
                parts.push(format!("<error>{err}</error>"));
            }
        }
        vec![ToolResultContentPart::Text {
            text: parts.join("\n\n"),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TaskOutputInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = [input.task_id.as_deref(), input.task_id_camel.as_deref()]
            .into_iter()
            .flatten()
            .find(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_default();
        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "task_id (or taskId) parameter is required".into(),
                error_code: None,
            });
        }
        let block = input.block;
        let timeout_ms = input.timeout.unwrap_or(30_000);

        // Stage 1: background task namespace.
        if let Some(handle) = ctx.task_handle.as_ref()
            && let Ok(initial) = handle.get_task_status(&task_id).await
        {
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
            let retrieval = if info.status.is_terminal() {
                RetrievalStatus::Success
            } else if block {
                RetrievalStatus::Timeout
            } else {
                RetrievalStatus::NotReady
            };
            let status_str = task_status_wire_string(info.status);
            return Ok(ToolResult {
                data: project_output_background(&info.id, status_str, output, None, retrieval),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
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
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

/// Wait for a background task to reach a terminal state. Event-
/// driven via [`TaskReader::subscribe_terminal`] — the production
/// impl returns a `watch::Receiver` that fires exactly once on the
/// `update_status` transition into Completed/Failed/Killed.
///
/// Falls back to a one-shot `get_task_status` snapshot if no
/// terminal subscription is available (test handles without watch
/// wiring). TS parity for the blocking semantics at
/// `TaskOutputTool.tsx:118-142` polling loop, but replaces 100 ms
/// polling with O(1) await — Rust has primitives JS lacks, no
/// reason to mirror its busy-wait.
async fn wait_for_task_completion(
    handle: &dyn coco_tool_runtime::TaskHandle,
    task_id: &str,
    initial: coco_types::TaskStateBase,
    timeout_ms: u64,
) -> coco_types::TaskStateBase {
    if initial.status.is_terminal() {
        return initial;
    }
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let Some(signal) = handle.subscribe_terminal(task_id).await else {
        return initial;
    };
    tokio::select! {
        _final_status = signal.await_terminal() => {
            handle.get_task_status(task_id).await.unwrap_or(initial)
        }
        _ = tokio::time::sleep(timeout) => initial,
    }
}

/// Wire-string projection of [`coco_types::TaskStatus`] used in the
/// `TaskOutput` envelope's `<status>` tag. TS parity with the lowercase
/// strings emitted at `TaskOutputTool.tsx:99-101`.
fn task_status_wire_string(status: coco_types::TaskStatus) -> &'static str {
    match status {
        coco_types::TaskStatus::Pending => "pending",
        coco_types::TaskStatus::Running => "running",
        coco_types::TaskStatus::Completed => "completed",
        coco_types::TaskStatus::Failed => "failed",
        coco_types::TaskStatus::Killed => "killed",
    }
}

// ── TodoWriteTool ─────────────────────────────────────────────────────

/// Typed input for [`TodoWriteTool`]. Items deserialize directly into
/// the existing `TodoRecord` type (already `Serialize +
/// Deserialize`); we add a `JsonSchema` derive at its definition.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TodoWriteInput {
    /// The updated todo list. Pass the full list each call; the
    /// prior list is replaced.
    #[serde(default)]
    pub todos: Vec<TodoRecord>,
}

pub struct TodoWriteTool;

#[async_trait::async_trait]
impl Tool for TodoWriteTool {
    type Input = TodoWriteInput;
    // Static schema from a literal `json!`; a parse failure means the literal
    // is malformed (a programmer error), so panicking on first build is correct.
    #[allow(clippy::expect_used)]
    fn runtime_validation_schema(&self) -> &coco_tool_runtime::ToolInputSchema {
        static SCHEMA: std::sync::OnceLock<coco_tool_runtime::ToolInputSchema> =
            std::sync::OnceLock::new();
        SCHEMA.get_or_init(|| {
            // Derive from `TodoWriteInput`, then inject the `status` enum
            // constraint: `TodoRecord.status` is a `String` in Rust (TUI/store
            // paths pre-date this typing pass) so schemars can't synthesize the
            // enum. TS `TodoItemSchema.status: z.enum([...])` restored here.
            let mut derived = coco_tool_runtime::derive_input_schema_value::<TodoWriteInput>();
            if let Some(status) = derived
                .pointer_mut("/properties/todos/items/properties/status")
                .and_then(serde_json::Value::as_object_mut)
            {
                status.insert(
                    "enum".into(),
                    serde_json::json!(["pending", "in_progress", "completed"]),
                );
            }
            let properties = derived
                .get("properties")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let required = derived
                .get("required")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            coco_tool_runtime::ToolInputSchema::from_static_value(serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": properties,
                "required": required,
            }))
        })
    }
    type Output = Value;

    fn to_auto_classifier_input(&self, input: &TodoWriteInput) -> Option<String> {
        Some(format!("{} items", input.todos.len()))
    }

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TodoWrite)
    }
    fn name(&self) -> &str {
        ToolName::TodoWrite.as_str()
    }
    fn description(&self, _input: &TodoWriteInput, _options: &DescriptionOptions) -> String {
        "Write or update the in-conversation TODO list. Pass the full list each call; \
         the prior list is replaced. Each item requires content, status, and activeForm."
            .into()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        !ctx.features.enabled(Feature::TaskV2)
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("write the per-agent todo checklist for tracking work")
    }

    /// TS parity: `TodoWriteTool.ts::mapToolResultToToolResultBlockParam`.
    /// The model only needs the success message + optional verification
    /// nudge — `oldTodos`/`newTodos` arrays are TUI/state concerns, not
    /// model-visible content. JSON-stringifying them wastes tokens.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let nudge_needed = data
            .get("verificationNudgeNeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let base = "Todos have been modified successfully. Ensure that you continue to use the todo list to track your progress. Please proceed with the current tasks if applicable";
        let text = if nudge_needed {
            format!(
                "{base}\n\nNOTE: You just closed out 3+ tasks and none of them was a verification step. Before writing your final summary, spawn the verification agent. You cannot self-assign PARTIAL by listing caveats in your summary — only the verifier issues a verdict."
            )
        } else {
            base.to_string()
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TodoWriteInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let incoming = input.todos;

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
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

#[cfg(test)]
#[path = "task_tools.test.rs"]
mod tests;
