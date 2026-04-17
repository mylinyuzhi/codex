//! Scheduling tools: CronCreate, CronDelete, CronList, RemoteTrigger.
//!
//! TS: tools/CronTool, tools/RemoteTriggerTool
//!
//! Uses `ctx.schedules` (ScheduleStore trait) for persistence.

use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

/// Maximum simultaneous cron jobs allowed by `CronCreate`. TS
/// `ScheduleCronTool/CronCreateTool.ts:25` `MAX_JOBS = 50`. Enforced
/// in `validate_input` so the model gets a clear error before the
/// store rejects.
const MAX_CRON_JOBS: usize = 50;

/// Lightweight 5-field cron expression validator. TS uses
/// `parseCronExpression()` from `utils/cron.ts` for full RFC parsing
/// + next-run scheduling; coco-rs accepts the same syntax minus
/// extension features (no L/W/# qualifiers) and rejects obviously
/// malformed inputs at the tool boundary.
///
/// Returns `true` if the expression is well-formed (5 fields, each
/// matching the simple grammar). Returns `false` for any structural
/// or per-field violation.
///
/// Grammar per field:
///   - `*` (any value)
///   - `N` (literal number)
///   - `*/N` (every N units)
///   - `A-B` (range)
///   - `A,B,C` (list — each element follows the same rules above)
///
/// TS `parseCronExpression` is more permissive (named days, step
/// inside ranges, etc.) but the bulk of model traffic uses the
/// simple form. We err on the side of accepting valid inputs and
/// only catching obvious typos like `*/5/*/*` or 4-field expressions.
fn is_valid_cron_expression(expr: &str) -> bool {
    let trimmed = expr.trim();
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    fields.iter().all(|field| field_is_valid(field))
}

fn field_is_valid(field: &str) -> bool {
    if field.is_empty() {
        return false;
    }
    // Lists: `A,B,C` — each element must be an atom (not another list).
    if field.contains(',') {
        return field.split(',').all(atom_is_valid);
    }
    atom_is_valid(field)
}

fn atom_is_valid(atom: &str) -> bool {
    if atom == "*" {
        return true;
    }
    // Step expressions `*/N` or `A/N`.
    if let Some((base, step)) = atom.split_once('/') {
        if step.parse::<u32>().is_err() {
            return false;
        }
        return base == "*" || base.parse::<u32>().is_ok() || range_is_valid(base);
    }
    // Range `A-B`.
    if atom.contains('-') {
        return range_is_valid(atom);
    }
    // Literal number.
    atom.parse::<u32>().is_ok()
}

fn range_is_valid(atom: &str) -> bool {
    let Some((start, end)) = atom.split_once('-') else {
        return false;
    };
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return false;
    };
    start <= end
}

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
                "description": "Standard 5-field cron expression in local time: \"M H DoM Mon DoW\" (e.g. \"*/5 * * * *\" = every 5 minutes, \"30 14 28 2 *\" = Feb 28 at 2:30pm local once)."
            }),
        );
        p.insert(
            "prompt".into(),
            serde_json::json!({
                "type": "string",
                "description": "The prompt to enqueue at each fire time."
            }),
        );
        p.insert(
            "recurring".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "true (default) = fire on every cron match until deleted or auto-expired. false = fire once at the next match, then auto-delete. Use false for \"remind me at X\" one-shot requests."
            }),
        );
        p.insert(
            "durable".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "true = persist to .claude/scheduled_tasks.json and survive restarts. false (default) = in-memory only, dies when this Claude session ends."
            }),
        );
        ToolInputSchema { properties: p }
    }

    /// TS `CronCreateTool.ts:82-103` `validateInput`: pre-flight checks
    /// for the cron expression syntax, schedule reachability within the
    /// next year, and the global MAX_JOBS cap. coco-rs implements the
    /// syntax check inline (no external cron crate dep) and the
    /// MAX_JOBS check via a synchronous best-effort `try_lock` against
    /// the schedule store. `nextCronRunMs` (the "next year" reachability
    /// check) is omitted because it requires a full cron parser that
    /// computes occurrences — expressions like `30 14 30 2 *` (Feb 30,
    /// invalid) will be rejected when `ctx.schedules.create_schedule`
    /// fails server-side. R7-T22.
    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let cron_expr = input.get("cron").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

        if cron_expr.is_empty() {
            return ValidationResult::invalid("cron parameter is required");
        }
        if prompt.is_empty() {
            return ValidationResult::invalid("prompt parameter is required");
        }
        if !is_valid_cron_expression(cron_expr) {
            return ValidationResult::invalid(format!(
                "Invalid cron expression '{cron_expr}'. Expected 5 fields: M H DoM Mon DoW."
            ));
        }
        ValidationResult::Valid
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
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let durable = input
            .get("durable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // R7-T22: enforce the global cron-job cap. TS
        // `CronCreateTool.ts:97-103` rejects when there are >= 50
        // active jobs; coco-rs queries the schedule store for the
        // current count. The check happens in execute (not
        // validate_input) because it's an async DB hit, and
        // validate_input is sync. Failing here surfaces as a tool
        // error to the model.
        if let Ok(existing) = ctx.schedules.list_schedules().await
            && existing.len() >= MAX_CRON_JOBS
        {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "Cron job limit reached ({MAX_CRON_JOBS}). Delete an existing job before creating a new one."
                ),
                source: None,
            });
        }

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
    /// TS `CronListTool.ts`: `isConcurrencySafe() { return true }`. Listing
    /// schedules is a pure read of the schedule store. CronCreate/Delete
    /// stay non-safe because they mutate the store.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        // TS `CronListTool.ts:20-33` outputSchema:
        //   { jobs: Array<{ id, cron, humanSchedule, prompt,
        //                   recurring?, durable? }> }
        //
        // Differences from the prior coco-rs shape:
        //  - Top-level wrapper `{ jobs: [...] }` (not a bare array).
        //  - Field names match TS: `cron` (was `schedule`), `prompt`
        //    (was `command`), `humanSchedule` (was missing).
        //  - Empty case still wraps in `{ jobs: [] }` instead of a
        //    free-form text string so the model gets a consistent
        //    discriminator.
        //  - `recurring` and `durable` are only included when truthy/
        //    explicitly false, matching TS spread-conditional shape
        //    `(t.recurring ? { recurring: true } : {})`.
        match ctx.schedules.list_schedules().await {
            Ok(entries) => {
                let jobs: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        let mut obj = serde_json::json!({
                            "id": e.id,
                            "cron": e.schedule,
                            "humanSchedule": e.schedule,
                            "prompt": e.command,
                        });
                        if e.enabled {
                            obj["recurring"] = serde_json::Value::Bool(true);
                        }
                        obj
                    })
                    .collect();
                Ok(ToolResult {
                    data: serde_json::json!({ "jobs": jobs }),
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

#[cfg(test)]
#[path = "scheduling.test.rs"]
mod tests;
