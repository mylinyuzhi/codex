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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    id: String,
    content: String,
    status: String,
}

fn generate_task_id() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    // Combine timestamp with a simple counter for uniqueness
    format!("task-{ts}")
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

        Ok(ToolResult {
            data: serde_json::to_value(&task).unwrap_or_default(),
            new_messages: vec![],
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
        match store.get(task_id) {
            Some(task) => Ok(ToolResult {
                data: serde_json::to_value(task).unwrap_or_default(),
                new_messages: vec![],
            }),
            None => Ok(ToolResult {
                data: serde_json::json!({"error": format!("Task '{task_id}' not found")}),
                new_messages: vec![],
            }),
        }
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
        let tasks: Vec<&Task> = store.values().collect();

        if tasks.is_empty() {
            return Ok(ToolResult {
                data: serde_json::json!({"tasks": [], "message": "No tasks found"}),
                new_messages: vec![],
            });
        }

        Ok(ToolResult {
            data: serde_json::to_value(&tasks).unwrap_or_default(),
            new_messages: vec![],
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
            return Ok(ToolResult {
                data: serde_json::json!({"error": format!("Task '{task_id}' not found")}),
                new_messages: vec![],
            });
        };

        // Update status if provided.
        if let Some(status_str) = input.get("status").and_then(|v| v.as_str()) {
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
        }

        if let Some(s) = input.get("subject").and_then(|v| v.as_str()) {
            task.subject = s.to_string();
        }
        if let Some(d) = input.get("description").and_then(|v| v.as_str()) {
            task.description = d.to_string();
        }
        if let Some(af) = input.get("activeForm").and_then(|v| v.as_str()) {
            task.active_form = Some(af.to_string());
        }
        if let Some(o) = input.get("owner").and_then(|v| v.as_str()) {
            task.owner = Some(o.to_string());
        }

        // Dependency graph: add blocks/blockedBy.
        if let Some(add_blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            for b in add_blocks {
                if let Some(id) = b.as_str() {
                    if !task.blocks.contains(&id.to_string()) {
                        task.blocks.push(id.to_string());
                    }
                }
            }
        }
        if let Some(add_blocked) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            for b in add_blocked {
                if let Some(id) = b.as_str() {
                    if !task.blocked_by.contains(&id.to_string()) {
                        task.blocked_by.push(id.to_string());
                    }
                }
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
        }

        let updated = task.clone();
        Ok(ToolResult {
            data: serde_json::to_value(&updated).unwrap_or_default(),
            new_messages: vec![],
        })
    }
}

// ── TaskStopTool ──

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
        "Stop a running task by its ID.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "taskId".into(),
            serde_json::json!({"type": "string", "description": "The task ID to stop"}),
        );
        ToolInputSchema { properties: p }
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
        match store.get_mut(task_id) {
            Some(task) => {
                task.status = TaskStatus::Stopped;
                let updated = task.clone();
                Ok(ToolResult {
                    data: serde_json::to_value(&updated).unwrap_or_default(),
                    new_messages: vec![],
                })
            }
            None => Ok(ToolResult {
                data: serde_json::json!({"error": format!("Task '{task_id}' not found")}),
                new_messages: vec![],
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
        "Read the output of a completed or running task.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "taskId".into(),
            serde_json::json!({"type": "string", "description": "The task ID to get output for"}),
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
        match store.get(task_id) {
            Some(task) => {
                let output = if task.output.is_empty() {
                    format!(
                        "Task '{}' has no output yet (status: {})",
                        task.id, task.status
                    )
                } else {
                    task.output.clone()
                };
                Ok(ToolResult {
                    data: serde_json::json!({"taskId": task.id, "status": task.status.to_string(), "output": output}),
                    new_messages: vec![],
                })
            }
            None => Ok(ToolResult {
                data: serde_json::json!({"error": format!("Task '{task_id}' not found")}),
                new_messages: vec![],
            }),
        }
    }
}

// ── TodoWriteTool ──

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
        "Write or update TODO items. Accepts an array of todo objects with id, content, and status."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "todos".into(),
            serde_json::json!({
                "type": "array",
                "description": "Array of TODO items",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "Unique todo ID"},
                        "content": {"type": "string", "description": "Todo content text"},
                        "status": {"type": "string", "enum": ["pending", "in_progress", "completed"], "description": "Todo status"}
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

        let mut store = get_todo_store()?;

        // Replace or insert each todo by id
        for incoming_todo in &incoming {
            if let Some(existing) = store.iter_mut().find(|t| t.id == incoming_todo.id) {
                existing.content.clone_from(&incoming_todo.content);
                existing.status.clone_from(&incoming_todo.status);
            } else {
                store.push(incoming_todo.clone());
            }
        }

        let count = store.len();
        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Updated {count} todo(s)"),
                "todos": serde_json::to_value(&*store).unwrap_or_default()
            }),
            new_messages: vec![],
        })
    }
}
