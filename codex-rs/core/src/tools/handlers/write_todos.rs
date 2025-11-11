use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::spec::JsonSchema;
use async_trait::async_trait;
use codex_protocol::protocol::EventMsg;
use codex_protocol::write_todos::TodoItem;
use codex_protocol::write_todos::TodoStatus;
use codex_protocol::write_todos::WriteTodosArgs;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct WriteTodosHandler;

// Comprehensive tool description adapted from gemini-cli
const WRITE_TODOS_DESCRIPTION: &str = r#"Write or update the active todos list for tracking execution progress.

This tool helps you list out the current subtasks required to complete a given user request. It helps you keep track of the current task, organize complex queries, and ensure you don't miss any steps.

## Task Statuses

- `pending`: Work has not begun on this task yet
- `in_progress`: Currently working on this task (mark just before beginning work)
- `completed`: Task has been successfully completed with no errors
- `cancelled`: Task is no longer required due to changing plans

## Core Methodology

1. **Use immediately**: As soon as you receive a complex user request, create an initial todo list
2. **Track everything**: Keep track of every subtask as you work through them
3. **Mark in-progress first**: Before starting a task, mark it as `in_progress` (only ONE at a time)
4. **Dynamic updates**: The list is NOT static - update it as plans evolve and new tasks emerge
5. **Mark completed**: Immediately mark tasks as `completed` when successfully finished
6. **Mark cancelled**: If tasks become unnecessary, mark them as `cancelled` instead of deleting
7. **Update frequently**: Don't batch updates - call this tool as soon as status changes

## When to USE this tool

✅ Complex tasks requiring multiple steps or phases
✅ Planning and organization for tasks more complex than simple Q&A
✅ Tasks where you want to show real-time progress to the user
✅ Breaking down ambiguous requirements into actionable subtasks

## When NOT to use this tool

❌ Simple tasks that can be completed in 1-2 steps
❌ If you can respond to the user in a single turn, this tool is not required
❌ Don't pad simple work with filler steps just to use the tool

## Important Constraints

- **Only ONE task can be `in_progress` at a time** - this is strictly enforced
- Each todo must have a non-empty description
- The entire list is replaced on each call (not incremental updates)
- Empty array clears the todo list

## Example Usage

User: "Refactor the authentication system to use JWT tokens"

Initial todos:
```json
{
  "todos": [
    {"description": "Analyze current authentication code", "status": "in_progress"},
    {"description": "Research JWT implementation best practices", "status": "pending"},
    {"description": "Design new token-based auth flow", "status": "pending"},
    {"description": "Implement JWT token generation", "status": "pending"},
    {"description": "Implement JWT token validation", "status": "pending"},
    {"description": "Update login endpoints", "status": "pending"},
    {"description": "Write integration tests", "status": "pending"},
    {"description": "Update API documentation", "status": "pending"}
  ]
}
```

After completing analysis:
```json
{
  "todos": [
    {"description": "Analyze current authentication code", "status": "completed"},
    {"description": "Research JWT implementation best practices", "status": "in_progress"},
    {"description": "Design new token-based auth flow", "status": "pending"},
    {"description": "Implement JWT token generation", "status": "pending"},
    {"description": "Implement JWT token validation", "status": "pending"},
    {"description": "Update login endpoints", "status": "pending"},
    {"description": "Write integration tests", "status": "pending"},
    {"description": "Update API documentation", "status": "pending"}
  ]
}
```

After discovering no documentation update needed:
```json
{
  "todos": [
    {"description": "Analyze current authentication code", "status": "completed"},
    {"description": "Research JWT implementation best practices", "status": "completed"},
    {"description": "Design new token-based auth flow", "status": "completed"},
    {"description": "Implement JWT token generation", "status": "in_progress"},
    {"description": "Implement JWT token validation", "status": "pending"},
    {"description": "Update login endpoints", "status": "pending"},
    {"description": "Write integration tests", "status": "pending"},
    {"description": "Update API documentation", "status": "cancelled"}
  ]
}
```

Remember: This tool is about execution tracking, not planning. Use `update_plan` for high-level strategy, and `write_todos` for granular progress tracking."#;

pub static WRITE_TODOS_TOOL: LazyLock<ToolSpec> = LazyLock::new(|| {
    // Define the status enum schema
    let status_schema = JsonSchema::String {
        description: Some(
            "Status of the todo: pending, in_progress, completed, or cancelled".to_string(),
        ),
    };

    // Define the todo item schema
    let mut todo_item_props = BTreeMap::new();
    todo_item_props.insert(
        "description".to_string(),
        JsonSchema::String {
            description: Some("Description of the task to be done".to_string()),
        },
    );
    todo_item_props.insert("status".to_string(), status_schema);

    let todo_item_schema = JsonSchema::Object {
        properties: todo_item_props,
        required: Some(vec!["description".to_string(), "status".to_string()]),
        additional_properties: Some(false.into()),
    };

    // Define the todos array schema
    let todos_array_schema = JsonSchema::Array {
        description: Some("Complete list of todos (replaces existing list)".to_string()),
        items: Box::new(todo_item_schema),
    };

    // Define the top-level parameters
    let mut properties = BTreeMap::new();
    properties.insert("todos".to_string(), todos_array_schema);

    ToolSpec::Function(ResponsesApiTool {
        name: "write_todos".to_string(),
        description: WRITE_TODOS_DESCRIPTION.to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["todos".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
});

#[async_trait]
impl ToolHandler for WriteTodosHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            call_id,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "write_todos handler received unsupported payload".to_string(),
                ));
            }
        };

        let content =
            handle_write_todos(session.as_ref(), turn.as_ref(), arguments, call_id).await?;

        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success: Some(true),
        })
    }
}

/// Handles the write_todos tool invocation by validating input and sending events to UI
pub(crate) async fn handle_write_todos(
    session: &Session,
    turn_context: &TurnContext,
    arguments: String,
    _call_id: String,
) -> Result<String, FunctionCallError> {
    let args = parse_write_todos_arguments(&arguments)?;

    // Validate todos
    validate_todos(&args.todos)?;

    // Send event to UI layer
    session
        .send_event(turn_context, EventMsg::TodoUpdate(args.clone()))
        .await;

    // Format response for LLM
    Ok(format_todo_response(&args.todos))
}

/// Parses the JSON arguments into WriteTodosArgs
fn parse_write_todos_arguments(arguments: &str) -> Result<WriteTodosArgs, FunctionCallError> {
    serde_json::from_str::<WriteTodosArgs>(arguments).map_err(|e| {
        FunctionCallError::RespondToModel(format!("Failed to parse todo arguments: {e}"))
    })
}

/// Validates the todos list according to the tool's constraints
fn validate_todos(todos: &[TodoItem]) -> Result<(), FunctionCallError> {
    // Check each todo has non-empty description
    for (idx, todo) in todos.iter().enumerate() {
        if todo.description.trim().is_empty() {
            return Err(FunctionCallError::RespondToModel(format!(
                "Todo at index {idx} has empty description"
            )));
        }
    }

    // Enforce single in_progress constraint
    let in_progress_count = todos
        .iter()
        .filter(|t| t.status == TodoStatus::InProgress)
        .count();

    if in_progress_count > 1 {
        return Err(FunctionCallError::RespondToModel(
            "Only one task can be 'in_progress' at a time.".to_string(),
        ));
    }

    Ok(())
}

/// Formats the todos list into a human-readable string for LLM feedback
fn format_todo_response(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "Successfully cleared the todo list.".to_string();
    }

    let formatted_todos = todos
        .iter()
        .enumerate()
        .map(|(idx, todo)| {
            let status_str = match todo.status {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
                TodoStatus::Cancelled => "cancelled",
            };
            format!("{}. [{status_str}] {}", idx + 1, todo.description)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("Successfully updated the todo list. The current list is now:\n{formatted_todos}")
}
