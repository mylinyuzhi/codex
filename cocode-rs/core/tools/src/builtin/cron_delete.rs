//! CronDelete tool for removing scheduled tasks.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_cron::CronJobStore;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct CronDeleteTool {
    store: CronJobStore,
}

impl CronDeleteTool {
    pub fn new(store: CronJobStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for CronDeleteTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::CronDelete.as_str()
    }

    fn description(&self) -> &str {
        prompts::CRON_DELETE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "jobId": {
                    "type": "string",
                    "description": "The job ID returned by CronCreate"
                }
            },
            "required": ["jobId"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
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
        let id = super::input_helpers::require_str(&input, "jobId")?;

        let snapshot = {
            let mut store = self.store.lock().await;
            if store.remove(id).is_none() {
                return Ok(ToolOutput::error(format!("Cron job not found: {id}")));
            }
            cocode_cron::jobs_to_value(&store)
        };

        // Persist durable jobs to disk (removed job may have been durable)
        if let Some(ref home) = ctx.paths.cocode_home
            && let Err(e) = cocode_cron::save_durable_jobs(&self.store, home).await
        {
            tracing::warn!(error = %e, "Failed to save durable cron jobs");
        }

        ctx.emit_progress(format!("Deleted cron job {id}")).await;

        Ok(
            ToolOutput::text(format!("Cron job {id} deleted successfully.")).with_modifier(
                cocode_protocol::ContextModifier::CronJobsUpdated { jobs: snapshot },
            ),
        )
    }
}

#[cfg(test)]
#[path = "cron_delete.test.rs"]
mod tests;
