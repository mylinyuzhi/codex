//! Scheduling tools: CronCreate, CronDelete, CronList, RemoteTrigger.
//!
//! TS: tools/CronTool, tools/RemoteTriggerTool
//!
//! Uses `ctx.schedules` (ScheduleStore trait) for persistence.

use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

pub struct CronCreateTool;

#[async_trait::async_trait]
impl Tool for CronCreateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronCreate)
    }
    fn name(&self) -> &str {
        ToolName::CronCreate.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Create a scheduled task that runs on a cron schedule.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "cron".into(),
            serde_json::json!({
                "type": "string",
                "description": "Standard 5-field cron expression in local time: \"M H DoM Mon DoW\""
            }),
        );
        p.insert(
            "prompt".into(),
            serde_json::json!({
                "type": "string",
                "description": "The prompt to enqueue at each fire time"
            }),
        );
        p.insert(
            "recurring".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "true (default) = fire on every cron match. false = fire once then auto-delete."
            }),
        );
        p.insert(
            "durable".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "true = persist across restarts. false (default) = in-memory only."
            }),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let cron_expr = input
            .get("cron")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let recurring = input
            .get("recurring")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let durable = input
            .get("durable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if cron_expr.is_empty() || prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "cron and prompt are required".into(),
                error_code: None,
            });
        }

        // Use cron expression as the schedule, prompt as the command
        let name = if recurring { "recurring" } else { "one-shot" };
        match ctx.schedules.create_schedule(name, cron_expr, prompt).await {
            Ok(entry) => Ok(ToolResult {
                data: serde_json::json!({
                    "id": entry.id,
                    "humanSchedule": entry.schedule,
                    "recurring": recurring,
                    "durable": durable,
                    "status": "created",
                }),
                new_messages: vec![],
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("Failed to create schedule: {e}"),
                source: None,
            }),
        }
    }
}

pub struct CronDeleteTool;

#[async_trait::async_trait]
impl Tool for CronDeleteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronDelete)
    }
    fn name(&self) -> &str {
        ToolName::CronDelete.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Delete a scheduled task by ID.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "schedule_id".into(),
            serde_json::json!({"type": "string", "description": "ID of the schedule to delete"}),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let id = input
            .get("schedule_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "schedule_id is required".into(),
                error_code: None,
            });
        }
        match ctx.schedules.delete_schedule(id).await {
            Ok(()) => Ok(ToolResult {
                data: serde_json::json!({"id": id, "status": "deleted"}),
                new_messages: vec![],
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("Failed to delete schedule {id}: {e}"),
                source: None,
            }),
        }
    }
}

pub struct CronListTool;

#[async_trait::async_trait]
impl Tool for CronListTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronList)
    }
    fn name(&self) -> &str {
        ToolName::CronList.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "List all scheduled tasks.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        match ctx.schedules.list_schedules().await {
            Ok(entries) if entries.is_empty() => Ok(ToolResult {
                data: serde_json::json!("No scheduled tasks"),
                new_messages: vec![],
            }),
            Ok(entries) => {
                let items: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.id, "name": e.name, "schedule": e.schedule,
                            "command": e.command, "enabled": e.enabled,
                        })
                    })
                    .collect();
                Ok(ToolResult {
                    data: serde_json::json!(items),
                    new_messages: vec![],
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("Failed to list schedules: {e}"),
                source: None,
            }),
        }
    }
}

pub struct RemoteTriggerTool;

#[async_trait::async_trait]
impl Tool for RemoteTriggerTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::RemoteTrigger)
    }
    fn name(&self) -> &str {
        ToolName::RemoteTrigger.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Manage remote trigger endpoints that can invoke agents.".into()
    }
    fn search_hint(&self) -> Option<&str> {
        Some("manage scheduled remote agent triggers")
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "action".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["list", "get", "create", "update", "run"],
                "description": "The action to perform on triggers"
            }),
        );
        p.insert(
            "trigger_id".into(),
            serde_json::json!({
                "type": "string",
                "description": "Trigger ID (required for get, update, and run)"
            }),
        );
        p.insert(
            "body".into(),
            serde_json::json!({
                "type": "object",
                "description": "JSON body for create and update actions"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, input: &Value) -> bool {
        matches!(
            input.get("action").and_then(|v| v.as_str()),
            Some("list" | "get")
        )
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> coco_tool::ValidationResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if action.is_empty() {
            return coco_tool::ValidationResult::invalid("action is required");
        }
        if !["list", "get", "create", "update", "run"].contains(&action) {
            return coco_tool::ValidationResult::invalid(
                "action must be one of: list, get, create, update, run",
            );
        }
        // get, update, run require trigger_id
        if matches!(action, "get" | "update" | "run")
            && input
                .get("trigger_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
        {
            return coco_tool::ValidationResult::invalid(
                "trigger_id is required for get, update, and run actions",
            );
        }
        coco_tool::ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let trigger_id = input.get("trigger_id").and_then(|v| v.as_str());
        let body = input.get("body");

        match action {
            "list" => match ctx.schedules.list_triggers().await {
                Ok(triggers) => Ok(ToolResult {
                    data: serde_json::to_value(&triggers).unwrap_or_default(),
                    new_messages: vec![],
                }),
                Err(e) => Err(ToolError::ExecutionFailed {
                    message: format!("Failed to list triggers: {e}"),
                    source: None,
                }),
            },
            "get" => {
                let id = trigger_id.unwrap_or_default();
                match ctx.schedules.get_trigger(id).await {
                    Ok(trigger) => Ok(ToolResult {
                        data: serde_json::to_value(&trigger).unwrap_or_default(),
                        new_messages: vec![],
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to get trigger {id}: {e}"),
                        source: None,
                    }),
                }
            }
            "create" => {
                let name = body
                    .and_then(|b| b.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed");
                let description = body
                    .and_then(|b| b.get("description"))
                    .and_then(|v| v.as_str());
                match ctx.schedules.create_trigger(name, description).await {
                    Ok(trigger) => Ok(ToolResult {
                        data: serde_json::json!({
                            "id": trigger.id,
                            "name": trigger.name,
                            "status": "created",
                        }),
                        new_messages: vec![],
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to create trigger: {e}"),
                        source: None,
                    }),
                }
            }
            "update" => {
                let id = trigger_id.unwrap_or_default();
                match ctx
                    .schedules
                    .update_trigger(id, body.cloned().unwrap_or(Value::Null))
                    .await
                {
                    Ok(trigger) => Ok(ToolResult {
                        data: serde_json::to_value(&trigger).unwrap_or_default(),
                        new_messages: vec![],
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to update trigger {id}: {e}"),
                        source: None,
                    }),
                }
            }
            "run" => {
                let id = trigger_id.unwrap_or_default();
                match ctx.schedules.run_trigger(id).await {
                    Ok(result) => Ok(ToolResult {
                        data: serde_json::json!({
                            "trigger_id": id,
                            "status": "triggered",
                            "result": result,
                        }),
                        new_messages: vec![],
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to run trigger {id}: {e}"),
                        source: None,
                    }),
                }
            }
            _ => Err(ToolError::InvalidInput {
                message: format!("Unknown action: {action}"),
                error_code: None,
            }),
        }
    }
}
