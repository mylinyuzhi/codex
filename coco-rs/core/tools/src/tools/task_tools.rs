use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// In-memory task storage, persists within a session.
static TASK_STORE: LazyLock<Mutex<HashMap<String, Task>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// In-memory todo storage, persists within a session.
static TODO_STORE: LazyLock<Mutex<Vec<TodoItem>>> = LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Stopped,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    subject: String,
    description: String,
    status: TaskStatus,
    output: String,
    /// Present continuous form shown in spinner (e.g., "Running tests").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_form: Option<String>,
    /// Owner agent or user ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    /// Task IDs that cannot start until this one completes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<String>,
    /// Task IDs that must complete before this one can start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    blocked_by: Vec<String>,
    /// Arbitrary metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,
}

/// TodoV1 list item, byte-matching the TS `TodoItemSchema` in
/// `utils/todo/types.ts`:
///
/// ```ts
/// { content: string (min 1), status: 'pending'|'in_progress'|'completed',
///   activeForm: string (min 1) }
/// ```
///
/// No `id` field — TS `TodoWriteTool` uses replace-all semantics scoped by
/// session/agent, so items are identified positionally. coco-rs matches
/// this exactly; the id-keyed merge behavior that predated this change has
/// been removed as a deliberate TS-alignment fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    content: String,
    status: String,
    #[serde(rename = "activeForm")]
    active_form: String,
}

/// Monotonic counter to avoid ID collisions between tasks created within
/// the same nanosecond (tests, batch inserts, etc).
static TASK_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn generate_task_id() -> String {
    use std::sync::atomic::Ordering;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    // Use nanosecond precision + an atomic counter so parallel test
    // threads never collide on the same ID. Millisecond-only precision
    // caused `HashMap::insert` races between tasks created in the same
    // tick, silently overwriting each other.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = TASK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("task-{ts}-{seq}")
}

fn get_store() -> Result<std::sync::MutexGuard<'static, HashMap<String, Task>>, ToolError> {
    TASK_STORE.lock().map_err(|e| ToolError::ExecutionFailed {
        message: format!("Failed to acquire task store lock: {e}"),
        source: None,
    })
}

fn get_todo_store() -> Result<std::sync::MutexGuard<'static, Vec<TodoItem>>, ToolError> {
    TODO_STORE.lock().map_err(|e| ToolError::ExecutionFailed {
        message: format!("Failed to acquire todo store lock: {e}"),
        source: None,
    })
}

// ── R5-T10: TS-shaped output projections ──────────────────────────────────
//
// TS task tools return intentionally minimal JSON shapes (`{task: {...}}`,
// `{tasks: [...]}` wrappers with filtered fields) so the model never sees
// internal bookkeeping fields like `output`, `active_form`, `metadata`,
// etc. These helpers convert the internal `Task` struct into the TS shape
// for each tool. Source-of-truth references live at:
//
//   TaskCreateTool.ts:36-43  — `{task: {id, subject}}`
//   TaskGetTool.ts:20-32     — `{task: {id, subject, description, status,
//                                       blocks, blockedBy} | null}`
//   TaskListTool.ts:16-28    — `{tasks: [{id, subject, status, owner?,
//                                        blockedBy}]}`  (+ _internal filter)
//   TaskUpdateTool.ts:69-83  — `{success, taskId, updatedFields, error?,
//                                statusChange?, verificationNudgeNeeded?}`
//   TaskOutputTool.tsx:51-54 — `{retrieval_status: 'success'|'timeout'|
//                                'not_ready', task: {task_id, task_type,
//                                status, description, output, exitCode?,
//                                error?, prompt?, result?}}`

/// Shape a freshly-created task for TaskCreate's `data`.
fn task_create_output(task: &Task) -> Value {
    serde_json::json!({
        "task": {
            "id": task.id,
            "subject": task.subject,
        }
    })
}

/// Shape a task lookup for TaskGet's `data`. `null` is used for misses to
/// match TS's `{task: ... | null}` schema rather than erroring.
fn task_get_output(task: Option<&Task>) -> Value {
    match task {
        Some(t) => serde_json::json!({
            "task": {
                "id": t.id,
                "subject": t.subject,
                "description": t.description,
                "status": t.status.to_string(),
                "blocks": t.blocks,
                "blockedBy": t.blocked_by,
            }
        }),
        None => serde_json::json!({ "task": null }),
    }
}

/// Shape one task for TaskList's `tasks[]` array — only 5 fields visible
/// to the model.
fn task_list_entry(task: &Task) -> Value {
    let mut obj = serde_json::json!({
        "id": task.id,
        "subject": task.subject,
        "status": task.status.to_string(),
        "blockedBy": task.blocked_by,
    });
    if let Some(owner) = &task.owner {
        obj["owner"] = serde_json::Value::String(owner.clone());
    }
    obj
}

/// TaskList also filters tasks whose metadata has `_internal` set — TS
/// uses this to hide session bookkeeping tasks from the model (see
/// `TaskListTool.ts:68-69`).
fn is_internal_task(task: &Task) -> bool {
    task.metadata.contains_key("_internal")
}

/// Shape a TaskUpdate response body. `updated_fields` is populated by the
/// update loop below; `status_change` is `Some((old, new))` when the
/// status changed.
fn task_update_output(
    task_id: &str,
    updated_fields: Vec<&'static str>,
    status_change: Option<(String, String)>,
) -> Value {
    let mut out = serde_json::json!({
        "success": true,
        "taskId": task_id,
        "updatedFields": updated_fields,
    });
    if let Some((from, to)) = status_change {
        out["statusChange"] = serde_json::json!({ "from": from, "to": to });
    }
    out
}

/// TaskUpdate error body (task not found). TS returns
/// `{success: false, taskId, error}`.
fn task_update_error_output(task_id: &str, error: &str) -> Value {
    serde_json::json!({
        "success": false,
        "taskId": task_id,
        "updatedFields": [],
        "error": error,
    })
}

/// TaskOutput retrieval status — derived from task terminal state +
/// whether a block-timeout hit.
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

/// Shape a TaskOutput response for a TODO-style task. TODO tasks are
/// updated synchronously by the model, so `task_type = "todo"`.
fn task_output_todo(task: &Task, status: RetrievalStatus) -> Value {
    serde_json::json!({
        "retrieval_status": status.as_str(),
        "task": {
            "task_id": task.id,
            "task_type": "todo",
            "status": task.status.to_string(),
            "description": task.description,
            "output": task.output,
        }
    })
}

/// Shape a TaskOutput response for a background task — this is populated
/// from `BackgroundTaskInfo` rather than the `Task` store.
fn task_output_background(
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
        task_obj["exitCode"] = serde_json::Value::Number(serde_json::Number::from(code));
    }
    serde_json::json!({
        "retrieval_status": status.as_str(),
        "task": task_obj,
    })
}

// ── TaskCreateTool ──

pub struct TaskCreateTool;

#[async_trait::async_trait]
impl Tool for TaskCreateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskCreate)
    }
    fn name(&self) -> &str {
        ToolName::TaskCreate.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Create a new background task with a subject and description.".into()
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
            serde_json::json!({"type": "string", "description": "Present continuous form for spinner (e.g., 'Running tests')"}),
        );
        p.insert(
            "metadata".into(),
            serde_json::json!({"type": "object", "description": "Arbitrary metadata to attach to the task"}),
        );
        ToolInputSchema { properties: p }
    }

    /// TS `TaskCreateTool.ts`: `isConcurrencySafe() { return true }`. Each
    /// create gets a unique nanosecond+counter ID (`generate_task_id`) so
    /// parallel inserts into `TASK_STORE` don't collide. The Mutex around
    /// the store handles the actual concurrent access.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
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

        // Parse initial metadata if provided
        let metadata = input
            .get("metadata")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let id = generate_task_id();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            status: TaskStatus::Pending,
            output: String::new(),
            active_form,
            owner: None,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            metadata,
        };

        let mut store = get_store()?;
        store.insert(id, task.clone());

        // TS: `TaskCreateTool.ts:36-43` — minimal `{task: {id, subject}}`.
        Ok(ToolResult {
            data: task_create_output(&task),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskGetTool ──

pub struct TaskGetTool;

#[async_trait::async_trait]
impl Tool for TaskGetTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskGet)
    }
    fn name(&self) -> &str {
        ToolName::TaskGet.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
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
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input.get("taskId").and_then(|v| v.as_str()).unwrap_or("");

        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "taskId parameter is required".into(),
                error_code: None,
            });
        }

        let store = get_store()?;
        // TS: `TaskGetTool.ts:20-32` — wrapped `{task: ... | null}` with
        // the minimal field set the model actually needs.
        Ok(ToolResult {
            data: task_get_output(store.get(task_id)),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskListTool ──

pub struct TaskListTool;

#[async_trait::async_trait]
impl Tool for TaskListTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskList)
    }
    fn name(&self) -> &str {
        ToolName::TaskList.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
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
        _input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let store = get_store()?;
        // TS: `TaskListTool.ts:68-69` filters `metadata._internal` tasks
        // so session bookkeeping doesn't leak into model-visible output.
        // `task_list_entry` projects each visible task into the TS 5-field
        // shape.
        let tasks: Vec<Value> = store
            .values()
            .filter(|t| !is_internal_task(t))
            .map(task_list_entry)
            .collect();

        Ok(ToolResult {
            data: serde_json::json!({ "tasks": tasks }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskUpdateTool ──

pub struct TaskUpdateTool;

#[async_trait::async_trait]
impl Tool for TaskUpdateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskUpdate)
    }
    fn name(&self) -> &str {
        ToolName::TaskUpdate.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
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
            serde_json::json!({"type": "string", "enum": ["pending", "in_progress", "completed"], "description": "New task status"}),
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
            serde_json::json!({"type": "array", "items": {"type": "string"}, "description": "Task IDs that cannot start until this one completes"}),
        );
        p.insert(
            "addBlockedBy".into(),
            serde_json::json!({"type": "array", "items": {"type": "string"}, "description": "Task IDs that must complete before this one can start"}),
        );
        p.insert(
            "metadata".into(),
            serde_json::json!({"type": "object", "description": "Metadata keys to merge (set key to null to delete)"}),
        );
        ToolInputSchema { properties: p }
    }

    /// TS `TaskUpdateTool.ts` flags this as concurrency-safe. Updates to
    /// different task IDs are independent; same-ID updates serialize
    /// through the `TASK_STORE` Mutex with last-writer-wins semantics.
    /// The model is responsible for not racing updates on the same task;
    /// the executor still allows the parallel batch.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let task_id = input.get("taskId").and_then(|v| v.as_str()).unwrap_or("");

        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "taskId parameter is required".into(),
                error_code: None,
            });
        }

        let mut store = get_store()?;
        let Some(task) = store.get_mut(task_id) else {
            // TS: `TaskUpdateTool.ts:69-83` — unknown id → `{success: false,
            // taskId, updatedFields: [], error}`.
            return Ok(ToolResult {
                data: task_update_error_output(task_id, &format!("Task '{task_id}' not found")),
                new_messages: vec![],
                app_state_patch: None,
            });
        };

        // Track which fields the call actually modified so we can return
        // them as `updatedFields`. Matches TS `TaskUpdateTool.ts` which
        // echoes back the set of keys the model touched.
        let mut updated_fields: Vec<&'static str> = Vec::new();
        let mut status_change: Option<(String, String)> = None;

        // Update status if provided.
        if let Some(status_str) = input.get("status").and_then(|v| v.as_str()) {
            let old_status = task.status.to_string();
            task.status = match status_str {
                "pending" => TaskStatus::Pending,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                "deleted" => TaskStatus::Stopped,
                other => {
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "Invalid status '{other}'. Must be pending, in_progress, completed, or deleted"
                        ),
                        error_code: None,
                    });
                }
            };
            let new_status = task.status.to_string();
            if old_status != new_status {
                status_change = Some((old_status, new_status));
            }
            updated_fields.push("status");
        }

        if let Some(s) = input.get("subject").and_then(|v| v.as_str()) {
            task.subject = s.to_string();
            updated_fields.push("subject");
        }
        if let Some(d) = input.get("description").and_then(|v| v.as_str()) {
            task.description = d.to_string();
            updated_fields.push("description");
        }
        if let Some(af) = input.get("activeForm").and_then(|v| v.as_str()) {
            task.active_form = Some(af.to_string());
            updated_fields.push("activeForm");
        }
        if let Some(o) = input.get("owner").and_then(|v| v.as_str()) {
            task.owner = Some(o.to_string());
            updated_fields.push("owner");
        }

        // Dependency graph: add blocks/blockedBy.
        if let Some(add_blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            let mut any_added = false;
            for b in add_blocks {
                if let Some(id) = b.as_str()
                    && !task.blocks.contains(&id.to_string())
                {
                    task.blocks.push(id.to_string());
                    any_added = true;
                }
            }
            if any_added {
                updated_fields.push("blocks");
            }
        }
        if let Some(add_blocked) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            let mut any_added = false;
            for b in add_blocked {
                if let Some(id) = b.as_str()
                    && !task.blocked_by.contains(&id.to_string())
                {
                    task.blocked_by.push(id.to_string());
                    any_added = true;
                }
            }
            if any_added {
                updated_fields.push("blockedBy");
            }
        }

        // Metadata: merge keys (null values delete the key).
        if let Some(meta) = input.get("metadata").and_then(|v| v.as_object()) {
            for (k, v) in meta {
                if v.is_null() {
                    task.metadata.remove(k);
                } else {
                    task.metadata.insert(k.clone(), v.clone());
                }
            }
            if !meta.is_empty() {
                updated_fields.push("metadata");
            }
        }

        // TS: `TaskUpdateTool.ts:69-83` — structured success response
        // with the list of updated fields and an optional `statusChange`.
        Ok(ToolResult {
            data: task_update_output(task_id, updated_fields, status_change),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ── TaskStopTool ──
//
// Unified stop entry point. Modeled on TS `TaskStopTool.ts:1-132` +
// `tasks/LocalShellTask/killShellTasks.ts:1-77`, which dispatches by task
// type: subagent → cancel via AgentHandle, background shell → kill the
// child process, TODO task → mark status = Stopped.
//
// Input schema accepts either `task_id` (preferred, new) or `shell_id`
// (deprecated KillShell compatibility — the old shell-only kill tool used
// that name before TS unified the interface).

pub struct TaskStopTool;

#[async_trait::async_trait]
impl Tool for TaskStopTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskStop)
    }
    fn name(&self) -> &str {
        ToolName::TaskStop.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Stop a running task by its ID. Works for both background shell tasks \
         (spawned via Bash with run_in_background=true) and TODO-style tasks \
         (created via TaskCreate)."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        // Match TS schema: support both `task_id` (canonical) and `shell_id`
        // (deprecated KillShell alias), plus legacy camelCase `taskId` for
        // backwards compat with existing coco-rs callers.
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
                "description": "Deprecated alias for task_id (KillShell compatibility). \
                               Prefer task_id."
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

    /// TS `TaskStopTool.ts`: `isConcurrencySafe() { return true }`. Stop is
    /// idempotent — calling it twice on the same ID is a no-op on the
    /// second call. Stops on different IDs are independent.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        // Accept task_id / shell_id / taskId — first non-empty wins.
        let task_id = input
            .get("task_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                input
                    .get("shell_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                input
                    .get("taskId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("")
            .to_string();

        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "task_id (or shell_id / taskId) parameter is required".into(),
                error_code: None,
            });
        }

        // Stage 1: try killing a live background task via TaskHandle. This
        // covers both shell background tasks (spawned by Bash with
        // run_in_background=true) and long-running subagent tasks — the
        // implementation layer dispatches based on the task type registered
        // in its internal registry.
        //
        // TS: `TaskStopTool.ts:117-120` routes to `stopTask()` which checks
        // the shell-task registry first, then the agent cancel-token registry.
        let mut stage1_result: Option<(bool, String)> = None;
        if let Some(task_handle) = ctx.task_handle.as_ref() {
            match task_handle.kill_task(&task_id).await {
                Ok(()) => {
                    stage1_result = Some((true, "killed background task".into()));
                }
                Err(e) => {
                    // Not a hard error — the ID might be for a TODO task
                    // instead. Remember the error for later reporting if
                    // Stage 2 also misses.
                    stage1_result = Some((false, e.to_string()));
                }
            }
        }

        // Stage 2: update in-memory TODO task store. TODO tasks and live
        // background tasks use separate ID namespaces, so we update whichever
        // one matches. It's normal for one stage to succeed and the other to
        // not find the ID — that's what "unified entry point" means.
        let mut todo_updated = false;
        if let Ok(mut store) = get_store()
            && let Some(task) = store.get_mut(&task_id)
        {
            task.status = TaskStatus::Stopped;
            todo_updated = true;
        }

        // Report result.
        //
        // TS `TaskStopTool.ts:107-130` `call()` throws an `Error` when
        // the task is missing (validation step at :74-79 already
        // rejects missing IDs via `validateInput` with errorCode: 1).
        // This surfaces as a tool ERROR to the model, not a successful
        // ToolResult with an `error` field.
        //
        // TS success output shape (`TaskStopTool.ts:122-130`):
        //   { message: string, task_id: string, task_type: string,
        //     command?: string }
        //
        // R3 fix: align with TS by returning `ToolError::ExecutionFailed`
        // for not-found cases (makes the model perceive it as a real
        // error so it can retry with a different ID) and matching the
        // TS success output shape.
        match (stage1_result, todo_updated) {
            // Background task was killed successfully.
            (Some((true, _)), _) => Ok(ToolResult {
                data: serde_json::json!({
                    "message": format!("Successfully stopped task: {task_id}"),
                    "task_id": task_id,
                    "task_type": "background",
                }),
                new_messages: vec![],
                app_state_patch: None,
            }),
            // TODO task was marked Stopped (and background task was
            // either not registered or the TaskHandle was absent).
            (_, true) => Ok(ToolResult {
                data: serde_json::json!({
                    "message": format!("Successfully stopped task: {task_id}"),
                    "task_id": task_id,
                    "task_type": "todo",
                }),
                new_messages: vec![],
                app_state_patch: None,
            }),
            // Neither stage found the task. Surface as a tool error so
            // the model knows to check its task ID and retry.
            (Some((false, err)), false) => Err(ToolError::ExecutionFailed {
                message: format!("No task found with ID: {task_id} ({err})"),
                source: None,
            }),
            (None, false) => Err(ToolError::ExecutionFailed {
                message: format!("No task found with ID: {task_id}"),
                source: None,
            }),
        }
    }
}

// ── TaskOutputTool ──

pub struct TaskOutputTool;

#[async_trait::async_trait]
impl Tool for TaskOutputTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TaskOutput)
    }
    fn name(&self) -> &str {
        ToolName::TaskOutput.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Read the output of a completed or running task. With block=true, \
         waits for the task to complete (or reach `timeout` milliseconds) \
         before returning. Works for both TODO tasks and background shell \
         tasks spawned via Bash(run_in_background=true)."
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
                               Set to false for an immediate snapshot. Only meaningful for \
                               background shell tasks — TODO tasks always return immediately."
            }),
        );
        p.insert(
            "timeout".into(),
            serde_json::json!({
                "type": "number",
                "description": "Blocking timeout in milliseconds (default 30000). Ignored \
                               when block=false. Polls the task every 100ms until the task \
                               completes or the timeout expires."
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
        // Accept `task_id` (canonical) or `taskId` (legacy), first non-empty wins.
        let task_id = input
            .get("task_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                input
                    .get("taskId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("")
            .to_string();

        if task_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "task_id (or taskId) parameter is required".into(),
                error_code: None,
            });
        }

        // TS: `TaskOutputTool.tsx:32` `block: semanticBoolean(z.boolean()
        // .default(true))`. Default is TRUE — the model usually wants to
        // wait for the task to finish rather than poll in a loop.
        let block = input
            .get("block")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        // TS timeout default: 30s (`TaskOutputTool.tsx:33`).
        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(30_000);

        // Stage 1: try the background-task namespace via TaskHandle. If
        // the ID resolves there, we can optionally block until completion
        // by polling `get_task_status`. TS `TaskOutputTool.tsx:218-238`
        // does the same thing via its own stall detector.
        if let Some(task_handle) = ctx.task_handle.as_ref() {
            // Probe once to see if this is a background-task ID.
            if let Ok(initial) = task_handle.get_task_status(&task_id).await {
                use coco_tool::BackgroundTaskStatus;
                let info = if block {
                    wait_for_task_completion(task_handle.as_ref(), &task_id, initial, timeout_ms)
                        .await
                } else {
                    initial
                };

                // Also fetch the latest output delta (from offset 0 —
                // we return the full accumulated output, not just new
                // bytes since the model's last call).
                let output = task_handle
                    .get_task_output_delta(&task_id, 0)
                    .await
                    .map(|d| d.content)
                    .unwrap_or_default();

                // Map background task status → TS `retrieval_status`.
                // TS distinguishes three states (`TaskOutputTool.tsx:229,
                // 236, 257`):
                //   - `success`  — task reached terminal state
                //   - `timeout`  — block window expired before terminal
                //   - `not_ready`— non-block call, task still running
                let retrieval = match info.status {
                    BackgroundTaskStatus::Completed
                    | BackgroundTaskStatus::Failed
                    | BackgroundTaskStatus::Killed => RetrievalStatus::Success,
                    _ if block => RetrievalStatus::Timeout,
                    _ => RetrievalStatus::NotReady,
                };
                let status_str = format!("{:?}", info.status).to_lowercase();
                // BackgroundTaskInfo doesn't currently expose exit_code —
                // TS emits it when available, but in coco-rs the shell
                // task returns only a status enum. Passing `None` keeps
                // the field absent in the output JSON, matching TS which
                // also marks `exitCode` as optional.
                let exit_code: Option<i32> = None;

                return Ok(ToolResult {
                    data: task_output_background(
                        &info.task_id,
                        &status_str,
                        output,
                        exit_code,
                        retrieval,
                    ),
                    new_messages: vec![],
                    app_state_patch: None,
                });
            }
        }

        // Stage 2: fall through to the TODO task store. TODO tasks are
        // updated synchronously by the model itself, so `block=true` has
        // no meaningful interpretation here — we return the current
        // snapshot regardless. If the caller wants blocking semantics,
        // they need to be operating on a real background task.
        let store = get_store()?;
        match store.get(&task_id) {
            Some(task) => {
                // TS `retrieval_status` mapping for TODO tasks: terminal
                // → `success`, non-terminal → `not_ready`. TODO tasks
                // don't time out (block has no effect), so we never
                // emit `timeout` here.
                let retrieval = match task.status {
                    TaskStatus::Completed | TaskStatus::Stopped => RetrievalStatus::Success,
                    _ => RetrievalStatus::NotReady,
                };
                Ok(ToolResult {
                    data: task_output_todo(task, retrieval),
                    new_messages: vec![],
                    app_state_patch: None,
                })
            }
            None => Ok(ToolResult {
                // TS shape for not-found: `{retrieval_status: 'not_ready',
                // task: null}` at `TaskOutputTool.tsx:53`.
                data: serde_json::json!({
                    "retrieval_status": RetrievalStatus::NotReady.as_str(),
                    "task": null,
                    "error": format!("Task '{task_id}' not found"),
                }),
                new_messages: vec![],
                app_state_patch: None,
            }),
        }
    }
}

/// Poll a background task until it reaches a terminal state or the
/// timeout expires. 100ms poll interval matches TS `TaskOutputTool.tsx:137`
/// `await sleep(100)`.
///
/// Returns the final `BackgroundTaskInfo` — either the terminal state
/// or the last observed snapshot when the timeout hit.
async fn wait_for_task_completion(
    handle: &dyn coco_tool::TaskHandle,
    task_id: &str,
    initial: coco_tool::BackgroundTaskInfo,
    timeout_ms: u64,
) -> coco_tool::BackgroundTaskInfo {
    use coco_tool::BackgroundTaskStatus;

    // Already terminal — no need to poll.
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
    // TS `TaskOutputTool.tsx:137` uses 100ms polling.
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
            Err(_) => {
                // Task vanished mid-poll (e.g. registry cleanup). Return
                // the last snapshot we had — the caller can detect the
                // status and report appropriately.
                return last;
            }
        }
    }

    last
}

// ── TodoWriteTool ──
//
// TS: `tools/TodoWriteTool/TodoWriteTool.ts:31-115`. The tool's purpose is
// to rewrite the session's in-conversation todo list from scratch on every
// invocation. The model sends the complete list, we overwrite storage,
// return `{ oldTodos, newTodos, verificationNudgeNeeded }`.
//
// The `allDone → clear` short-circuit matches TS lines 69-70: when every
// item is `completed`, the list is cleared so the model sees a fresh
// slate next turn rather than being tempted to keep appending "done"
// items.

pub struct TodoWriteTool;

#[async_trait::async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TodoWrite)
    }
    fn name(&self) -> &str {
        ToolName::TodoWrite.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Write or update the in-conversation TODO list. Pass the full list each call; \
         the prior list is replaced. Each item requires content, status, and activeForm."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        // Byte-match TS `utils/todo/types.ts` `TodoItemSchema`. No `id`
        // field (TS doesn't have one) — use positional identity.
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
                        "content": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Task description"
                        },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"],
                            "description": "Task status"
                        },
                        "activeForm": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Verb-phrase shown while the task is in progress"
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
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let todos_value = input.get("todos").cloned().unwrap_or(Value::Array(vec![]));

        let incoming: Vec<TodoItem> =
            serde_json::from_value(todos_value).map_err(|e| ToolError::InvalidInput {
                message: format!("Invalid todos format: {e}"),
                error_code: None,
            })?;

        // Field validation: TS schema enforces min-length 1 via zod; mirror
        // that on the Rust side so malformed inputs error out before we
        // clobber the store.
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

        // Capture the prior list before we overwrite so we can return it
        // as `oldTodos` — TS does the same at `TodoWriteTool.ts:68`.
        let (old_todos_value, new_store_contents) = {
            let mut store = get_todo_store()?;
            let old = serde_json::to_value(store.clone()).unwrap_or_default();

            // `allDone → clear` short-circuit (TS line 69-70).
            let all_done = !incoming.is_empty() && incoming.iter().all(|t| t.status == "completed");
            if all_done {
                store.clear();
            } else {
                *store = incoming.clone();
            }
            (old, store.clone())
        };

        // TS always returns the raw `todos` that the model sent as
        // `newTodos`, not the stored snapshot — that way the model sees
        // its own input echoed back even when `allDone` cleared the store
        // (TS line 99: `newTodos: todos`).
        let _ = new_store_contents; // silence unused warning — storage is observed via get_todo_store elsewhere
        Ok(ToolResult {
            data: serde_json::json!({
                "oldTodos": old_todos_value,
                "newTodos": serde_json::to_value(&incoming).unwrap_or_default(),
                "verificationNudgeNeeded": false,
            }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "task_tools.test.rs"]
mod tests;
