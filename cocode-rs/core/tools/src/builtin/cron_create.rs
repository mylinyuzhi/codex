//! CronCreate tool for scheduling recurring tasks.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_cron::CronJob;
use cocode_cron::CronJobStatus;
use cocode_cron::CronJobStore;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct CronCreateTool {
    store: CronJobStore,
}

impl CronCreateTool {
    pub fn new(store: CronJobStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::CronCreate.as_str()
    }

    fn description(&self) -> &str {
        prompts::CRON_CREATE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "cron": {
                    "type": "string",
                    "description": "Standard 5-field cron expression in local time (minute hour day-of-month month day-of-week), or a simple interval like '5m', '1h', '30s'. Example: '*/10 * * * *' for every 10 minutes"
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt or command to execute on each trigger"
                },
                "description": {
                    "type": "string",
                    "description": "Optional human-readable description of the job"
                },
                "recurring": {
                    "type": "boolean",
                    "description": "Whether this job recurs (true, default) or executes once then auto-deletes (false)",
                    "default": true
                },
                "durable": {
                    "type": "boolean",
                    "description": "Persist across sessions to .cocode/scheduled_tasks.json (default: false)",
                    "default": false
                }
            },
            "required": ["cron", "prompt"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn should_defer(&self) -> bool {
        true
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Cron)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let cron_input = super::input_helpers::require_str(&input, "cron")?;
        let prompt = super::input_helpers::require_str(&input, "prompt")?;

        // Parse simple interval format (e.g., "5m", "1h", "30s") or validate cron expression
        let schedule = cocode_cron::parse_schedule(cron_input)
            .map_err(|msg| crate::error::tool_error::InvalidInputSnafu { message: msg }.build())?;

        let recurring = super::input_helpers::bool_or(&input, "recurring", true);

        let job_id = cocode_cron::generate_cron_id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Auto-expiry for recurring jobs (3 days)
        let expires_at = if recurring {
            Some(now + cocode_cron::DEFAULT_RECURRING_EXPIRY_SECS)
        } else {
            None
        };

        let job = CronJob {
            id: job_id.clone(),
            cron: schedule.clone(),
            prompt: prompt.to_string(),
            description: input["description"].as_str().map(String::from),
            recurring,
            durable: super::input_helpers::bool_or(&input, "durable", false),
            created_at: now,
            execution_count: 0,
            last_executed_at: None,
            expires_at,
            status: CronJobStatus::Active,
            consecutive_failures: 0,
            next_fire_at: None,
        };

        let snapshot = {
            let mut store = self.store.lock().await;

            // Enforce max job limit
            let active_count = store
                .values()
                .filter(|j| j.status == CronJobStatus::Active)
                .count();
            if active_count >= cocode_cron::DEFAULT_MAX_JOBS as usize {
                return Ok(ToolOutput::error(format!(
                    "Maximum of {} active cron jobs reached. Delete some jobs first.",
                    cocode_cron::DEFAULT_MAX_JOBS as usize
                )));
            }

            store.insert(job_id.clone(), job);
            cocode_cron::jobs_to_value(&store)
        };

        // Persist durable jobs to disk
        if super::input_helpers::bool_or(&input, "durable", false)
            && let Some(ref home) = ctx.cocode_home
            && let Err(e) = cocode_cron::save_durable_jobs(&self.store, home).await
        {
            tracing::warn!(error = %e, "Failed to save durable cron jobs");
        }

        ctx.emit_progress(format!("Created cron job {job_id}: {schedule}"))
            .await;

        let type_note = if recurring { "" } else { " (one-shot)" };

        Ok(ToolOutput::text(format!(
            "Cron job created successfully{type_note}.\nID: {job_id}\nSchedule: {schedule}\nPrompt: {prompt}",
        ))
        .with_modifier(cocode_protocol::ContextModifier::CronJobsUpdated {
            jobs: snapshot,
        }))
    }
}

#[cfg(test)]
#[path = "cron_create.test.rs"]
mod tests;
