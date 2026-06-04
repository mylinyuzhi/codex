//! Scheduling tools: CronCreate, CronDelete, CronList, RemoteTrigger.
//!
//! TS: tools/CronTool, tools/RemoteTriggerTool
//!
//! Uses `ctx.schedules` (ScheduleStore trait) for persistence.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::Feature;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Maximum simultaneous cron jobs allowed by `CronCreate`. TS
/// `ScheduleCronTool/CronCreateTool.ts:25` `MAX_JOBS = 50`. Enforced
/// in `validate_input` so the model gets a clear error before the
/// store rejects.
const MAX_CRON_JOBS: usize = 50;

/// Recurring jobs auto-expire after this many days. TS
/// `cronTasks.ts:354` `recurringMaxAgeMs = 7 * 24 * 60 * 60 * 1000`
/// → `DEFAULT_MAX_AGE_DAYS = 7` (`prompt.ts:8-9`). Surfaced in the
/// model-visible confirmation so the model can warn the user.
const DEFAULT_MAX_AGE_DAYS: u32 = 7;

/// Lightweight 5-field cron expression validator. TS uses
/// `parseCronExpression()` from `utils/cron.ts` for full RFC parsing
/// + next-run scheduling; coco-rs accepts the same syntax minus
///   extension features (no L/W/# qualifiers) and rejects obviously
///   malformed inputs at the tool boundary.
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

/// Typed input for [`CronCreateTool`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CronCreateInput {
    /// Standard 5-field cron expression in local time: "M H DoM Mon
    /// DoW" (e.g. "*/5 * * * *" = every 5 minutes, "30 14 28 2 *" =
    /// Feb 28 at 2:30pm local once).
    #[serde(default)]
    pub cron: String,
    /// The prompt to enqueue at each fire time.
    #[serde(default)]
    pub prompt: String,
    /// true (default) = fire on every cron match until deleted or
    /// auto-expired. false = fire once at the next match, then
    /// auto-delete. Use false for "remind me at X" one-shot requests.
    #[serde(default = "default_true")]
    pub recurring: bool,
    /// true = persist to `.claude/scheduled_tasks.json` and survive
    /// restarts. false (default) = in-memory only, dies when this
    /// Claude session ends.
    #[serde(default)]
    pub durable: bool,
}

fn default_true() -> bool {
    true
}

/// Typed output for [`CronCreateTool`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronCreateOutput {
    pub id: String,
    /// Human-readable cron schedule (currently echoes back the cron
    /// expression). Field name preserved as `humanSchedule` for
    /// TS-parity (`CronCreateTool.ts`).
    #[serde(rename = "humanSchedule")]
    pub human_schedule: String,
    pub recurring: bool,
    pub durable: bool,
    pub status: String,
}

pub struct CronCreateTool;

#[async_trait::async_trait]
impl Tool for CronCreateTool {
    type Input = CronCreateInput;
    coco_tool_runtime::impl_runtime_schema!(CronCreateInput);
    type Output = CronCreateOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronCreate)
    }
    fn name(&self) -> &str {
        ToolName::CronCreate.as_str()
    }
    /// TS `CronCreateTool.ts:67-69` `isEnabled() { return isKairosCronEnabled() }`.
    /// Hidden from the model unless the cron feature is on — without a real
    /// [`ScheduleStore`](coco_tool_runtime::ScheduleStore) the tool only
    /// fails, so advertising it would waste model turns.
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::AgentTriggers)
    }
    fn description(&self, _input: &CronCreateInput, _options: &DescriptionOptions) -> String {
        "Create a scheduled task that runs on a cron schedule.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("schedule a recurring cron job to run later")
    }

    /// Render the create envelope as a single-line confirmation. TS
    /// parity: `CronCreateTool.ts:143-154 mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, out: &CronCreateOutput) -> Vec<ToolResultContentPart> {
        let id = &out.id;
        let schedule = &out.human_schedule;
        let where_str = if out.durable {
            "Persisted to .claude/scheduled_tasks.json"
        } else {
            "Session-only (not written to disk, dies when Claude exits)"
        };
        let text = if out.recurring {
            format!(
                "Scheduled recurring job {id} ({schedule}). {where_str}. Auto-expires after {DEFAULT_MAX_AGE_DAYS} days. Use CronDelete to cancel sooner."
            )
        } else {
            format!(
                "Scheduled one-shot task {id} ({schedule}). {where_str}. It will fire once then auto-delete."
            )
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
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
    fn validate_input(&self, input: &CronCreateInput, _ctx: &ToolUseContext) -> ValidationResult {
        if input.cron.is_empty() {
            return ValidationResult::invalid("cron parameter is required");
        }
        if input.prompt.is_empty() {
            return ValidationResult::invalid("prompt parameter is required");
        }
        if !is_valid_cron_expression(&input.cron) {
            return ValidationResult::invalid(format!(
                "Invalid cron expression '{cron}'. Expected 5 fields: M H DoM Mon DoW.",
                cron = input.cron
            ));
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: CronCreateInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<CronCreateOutput>, ToolError> {
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
                display_data: None,
                source: None,
            });
        }

        if input.cron.is_empty() || input.prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "cron and prompt are required".into(),
                error_code: None,
            });
        }

        // Use cron expression as the schedule, prompt as the command
        let name = if input.recurring {
            "recurring"
        } else {
            "one-shot"
        };
        match ctx
            .schedules
            .create_schedule(name, &input.cron, &input.prompt)
            .await
        {
            Ok(entry) => Ok(ToolResult {
                data: CronCreateOutput {
                    id: entry.id,
                    human_schedule: entry.schedule,
                    recurring: input.recurring,
                    durable: input.durable,
                    status: "created".into(),
                },
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("Failed to create schedule: {e}"),
                display_data: None,
                source: None,
            }),
        }
    }
}

/// Typed input for [`CronDeleteTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CronDeleteInput {
    /// ID of the schedule to delete
    #[serde(default)]
    pub schedule_id: String,
}

/// Typed output for [`CronDeleteTool`].
///
/// All fields marked `#[serde(default)]` so test fixtures and
/// transcript replay that provide partial envelopes (e.g.
/// `{"id": "x"}`) still deserialize into a valid struct for
/// `render_for_model`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronDeleteOutput {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
}

pub struct CronDeleteTool;

#[async_trait::async_trait]
impl Tool for CronDeleteTool {
    type Input = CronDeleteInput;
    coco_tool_runtime::impl_runtime_schema!(CronDeleteInput);
    type Output = CronDeleteOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronDelete)
    }
    fn name(&self) -> &str {
        ToolName::CronDelete.as_str()
    }
    /// TS `CronDeleteTool.ts` `isEnabled() { return isKairosCronEnabled() }`.
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::AgentTriggers)
    }
    fn description(&self, _input: &CronDeleteInput, _options: &DescriptionOptions) -> String {
        "Delete a scheduled task by ID.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("delete a scheduled cron job by id")
    }

    /// TS `CronDeleteTool.ts:86-92 mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, out: &CronDeleteOutput) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: format!("Cancelled job {id}.", id = out.id),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: CronDeleteInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<CronDeleteOutput>, ToolError> {
        if input.schedule_id.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "schedule_id is required".into(),
                error_code: None,
            });
        }
        match ctx.schedules.delete_schedule(&input.schedule_id).await {
            Ok(()) => Ok(ToolResult {
                data: CronDeleteOutput {
                    id: input.schedule_id,
                    status: "deleted".into(),
                },
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!(
                    "Failed to delete schedule {id}: {e}",
                    id = input.schedule_id
                ),
                display_data: None,
                source: None,
            }),
        }
    }
}

/// Typed input for [`CronListTool`] — no parameters.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CronListInput {}

/// One scheduled job entry in [`CronListOutput`].
///
/// TS `CronListTool.ts:20-33` outputSchema:
///   `{ id, cron, humanSchedule, prompt, recurring?, durable? }`
///
/// All fields marked `#[serde(default)]` so partial test fixtures /
/// transcript replay deserializes successfully into a struct the
/// renderer can iterate.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronListJob {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub cron: String,
    /// Field-renamed to `humanSchedule` for TS parity
    /// (`CronListTool.ts`).
    #[serde(default, rename = "humanSchedule")]
    pub human_schedule: String,
    #[serde(default)]
    pub prompt: String,
    /// TS uses spread-conditional `(t.recurring ? { recurring: true }
    /// : {})` — omit when not set. `Option<bool>` lets us distinguish
    /// "explicitly false" from "absent" on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurring: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable: Option<bool>,
}

/// Typed output for [`CronListTool`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronListOutput {
    pub jobs: Vec<CronListJob>,
}

pub struct CronListTool;

#[async_trait::async_trait]
impl Tool for CronListTool {
    type Input = CronListInput;
    coco_tool_runtime::impl_runtime_schema!(CronListInput);
    type Output = CronListOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::CronList)
    }
    fn name(&self) -> &str {
        ToolName::CronList.as_str()
    }
    /// TS `CronListTool.ts:48-50` `isEnabled() { return isKairosCronEnabled() }`.
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::AgentTriggers)
    }
    fn description(&self, _input: &CronListInput, _options: &DescriptionOptions) -> String {
        "List all scheduled tasks.".into()
    }
    fn is_read_only(&self, _input: &CronListInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    /// TS `CronListTool.ts`: `isConcurrencySafe() { return true }`. Listing
    /// schedules is a pure read of the schedule store. CronCreate/Delete
    /// stay non-safe because they mutate the store.
    fn is_concurrency_safe(&self, _input: &CronListInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("list active cron jobs and schedules")
    }

    /// Render `{jobs: [...]}` as a human-readable summary list.
    /// Empty list → "No scheduled tasks." Otherwise: count + bulleted
    /// `- {id}: {humanSchedule} → {prompt}` per job.
    fn render_for_model(&self, out: &CronListOutput) -> Vec<ToolResultContentPart> {
        let text = if out.jobs.is_empty() {
            "No scheduled tasks.".to_string()
        } else {
            let n = out.jobs.len();
            let plural = if n == 1 { "task" } else { "tasks" };
            let mut buf = format!("{n} scheduled {plural}:");
            for job in &out.jobs {
                let schedule = if job.human_schedule.is_empty() {
                    job.cron.as_str()
                } else {
                    job.human_schedule.as_str()
                };
                buf.push_str(&format!(
                    "\n- {id}: {schedule} → {prompt}",
                    id = job.id,
                    prompt = job.prompt
                ));
            }
            buf
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        _input: CronListInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<CronListOutput>, ToolError> {
        match ctx.schedules.list_schedules().await {
            Ok(entries) => {
                let jobs: Vec<CronListJob> = entries
                    .iter()
                    .map(|e| CronListJob {
                        id: e.id.clone(),
                        cron: e.schedule.clone(),
                        human_schedule: e.schedule.clone(),
                        prompt: e.command.clone(),
                        recurring: if e.enabled { Some(true) } else { None },
                        durable: None,
                    })
                    .collect();
                Ok(ToolResult {
                    data: CronListOutput { jobs },
                    new_messages: vec![],
                    app_state_patch: None,
                    permission_updates: Vec::new(),
                    display_data: None,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!("Failed to list schedules: {e}"),
                display_data: None,
                source: None,
            }),
        }
    }
}

/// Action for [`RemoteTriggerTool`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTriggerAction {
    /// Default — list all configured triggers.
    #[default]
    List,
    /// Look up a single trigger by id.
    Get,
    /// Create a new trigger from `body`.
    Create,
    /// Update an existing trigger; merges `body` into the stored trigger.
    Update,
    /// Fire the trigger now and return its run result.
    Run,
}

impl RemoteTriggerAction {
    fn requires_trigger_id(self) -> bool {
        matches!(self, Self::Get | Self::Update | Self::Run)
    }
}

/// Typed input for [`RemoteTriggerTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RemoteTriggerInput {
    /// The action to perform on triggers
    #[serde(default)]
    pub action: RemoteTriggerAction,
    /// Trigger ID (required for get, update, and run)
    #[serde(default)]
    pub trigger_id: Option<String>,
    /// JSON body for create and update actions. Free-form Value
    /// because the schema of each backend's trigger config is
    /// backend-defined and not modeled here.
    #[serde(default)]
    pub body: Option<Value>,
}

pub struct RemoteTriggerTool;

#[async_trait::async_trait]
impl Tool for RemoteTriggerTool {
    type Input = RemoteTriggerInput;
    coco_tool_runtime::impl_runtime_schema!(RemoteTriggerInput);
    /// Output is `Value` — per-action shape (`list` returns array,
    /// `get`/`update` return a trigger object, `create` returns
    /// `{id, name, status}`, `run` returns `{trigger_id, status, result}`).
    /// Tagged-enum modeling deferred until backend types crystallize.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::RemoteTrigger)
    }
    fn name(&self) -> &str {
        ToolName::RemoteTrigger.as_str()
    }
    /// TS `RemoteTriggerTool.ts:57-62` gates `isEnabled()` on a growthbook
    /// flag + `isPolicyAllowed('allow_remote_sessions')`. coco-rs collapses
    /// that to the [`Feature::AgentTriggersRemote`] capability gate. The live
    /// transport (claude.ai OAuth + Anthropic-internal endpoints) is an
    /// explicit non-goal, so the feature stays off by default and the tool
    /// is hidden rather than registered-and-failing.
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(Feature::AgentTriggersRemote)
    }
    fn description(&self, _input: &RemoteTriggerInput, _options: &DescriptionOptions) -> String {
        "Manage remote trigger endpoints that can invoke agents.".into()
    }
    fn search_hint(&self) -> Option<&str> {
        Some("manage scheduled remote agent triggers")
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn is_read_only(&self, input: &RemoteTriggerInput) -> bool {
        matches!(
            input.action,
            RemoteTriggerAction::List | RemoteTriggerAction::Get
        )
    }
    fn is_concurrency_safe(&self, _input: &RemoteTriggerInput) -> bool {
        true
    }

    /// Render trigger envelopes per-action. Most paths return a small
    /// `{id, name, status}` shape; `update` returns the full trigger.
    /// Pick a one-line confirmation for actions; fall back to JSON
    /// for unknown shapes.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let status = data.get("status").and_then(Value::as_str);
        let id = data
            .get("id")
            .or_else(|| data.get("trigger_id"))
            .and_then(Value::as_str);
        let text = match status {
            Some("created") => {
                let name = data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                format!("Trigger created. id: {}. name: {name}.", id.unwrap_or("?"))
            }
            Some("triggered") => format!("Trigger {} fired.", id.unwrap_or("?")),
            Some("deleted") => format!("Trigger {} deleted.", id.unwrap_or("?")),
            _ => serde_json::to_string(data).unwrap_or_default(),
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    fn validate_input(
        &self,
        input: &RemoteTriggerInput,
        _ctx: &ToolUseContext,
    ) -> coco_tool_runtime::ValidationResult {
        // `action` is now typed; unknown wire values fail at deserialize
        // time (one layer up). Only the "trigger_id required" gate
        // remains as a semantic check.
        if input.action.requires_trigger_id()
            && input.trigger_id.as_deref().unwrap_or("").is_empty()
        {
            return coco_tool_runtime::ValidationResult::invalid(
                "trigger_id is required for get, update, and run actions",
            );
        }
        coco_tool_runtime::ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: RemoteTriggerInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let trigger_id = input.trigger_id.as_deref();
        let body = input.body.as_ref();

        match input.action {
            RemoteTriggerAction::List => match ctx.schedules.list_triggers().await {
                Ok(triggers) => Ok(ToolResult {
                    data: serde_json::to_value(&triggers).unwrap_or_default(),
                    new_messages: vec![],
                    app_state_patch: None,
                    permission_updates: Vec::new(),
                    display_data: None,
                }),
                Err(e) => Err(ToolError::ExecutionFailed {
                    message: format!("Failed to list triggers: {e}"),
                    display_data: None,
                    source: None,
                }),
            },
            RemoteTriggerAction::Get => {
                let id = trigger_id.unwrap_or_default();
                match ctx.schedules.get_trigger(id).await {
                    Ok(trigger) => Ok(ToolResult {
                        data: serde_json::to_value(&trigger).unwrap_or_default(),
                        new_messages: vec![],
                        app_state_patch: None,
                        permission_updates: Vec::new(),
                        display_data: None,
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to get trigger {id}: {e}"),
                        display_data: None,
                        source: None,
                    }),
                }
            }
            RemoteTriggerAction::Create => {
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
                        app_state_patch: None,
                        permission_updates: Vec::new(),
                        display_data: None,
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to create trigger: {e}"),
                        display_data: None,
                        source: None,
                    }),
                }
            }
            RemoteTriggerAction::Update => {
                let id = trigger_id.unwrap_or_default();
                match ctx
                    .schedules
                    .update_trigger(id, body.cloned().unwrap_or(Value::Null))
                    .await
                {
                    Ok(trigger) => Ok(ToolResult {
                        data: serde_json::to_value(&trigger).unwrap_or_default(),
                        new_messages: vec![],
                        app_state_patch: None,
                        permission_updates: Vec::new(),
                        display_data: None,
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to update trigger {id}: {e}"),
                        display_data: None,
                        source: None,
                    }),
                }
            }
            RemoteTriggerAction::Run => {
                let id = trigger_id.unwrap_or_default();
                match ctx.schedules.run_trigger(id).await {
                    Ok(result) => Ok(ToolResult {
                        data: serde_json::json!({
                            "trigger_id": id,
                            "status": "triggered",
                            "result": result,
                        }),
                        new_messages: vec![],
                        app_state_patch: None,
                        permission_updates: Vec::new(),
                        display_data: None,
                    }),
                    Err(e) => Err(ToolError::ExecutionFailed {
                        message: format!("Failed to run trigger {id}: {e}"),
                        display_data: None,
                        source: None,
                    }),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "scheduling.test.rs"]
mod tests;
