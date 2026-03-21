//! CronList tool for listing scheduled tasks.

use super::cron_state::CronJobStore;
use super::cron_state::{self};
use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct CronListTool {
    store: CronJobStore,
}

impl CronListTool {
    pub fn new(store: CronJobStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::CronList.as_str()
    }

    fn description(&self) -> &str {
        prompts::CRON_LIST_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "include_completed": {
                    "type": "boolean",
                    "description": "Include completed and expired jobs in the listing (default: false)",
                    "default": false
                }
            }
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Cron)
    }

    async fn execute(&self, input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        let include_completed = super::input_helpers::bool_or(&input, "include_completed", false);
        let store = self.store.lock().await;
        if include_completed {
            let summary = cron_state::format_cron_summary(&store);
            Ok(ToolOutput::text(summary))
        } else {
            // Filter to active jobs only — build summary directly to avoid cloning
            if store
                .values()
                .all(|j| j.status != cron_state::CronJobStatus::Active)
            {
                return Ok(ToolOutput::text("No scheduled jobs.".to_string()));
            }
            let mut output = String::new();
            for job in store.values() {
                if job.status != cron_state::CronJobStatus::Active {
                    continue;
                }
                let type_marker = if job.recurring { "" } else { " (one-shot)" };
                let durable_marker = if job.durable { " [durable]" } else { "" };
                output.push_str(&format!(
                    "- {}: [{}]{}{}\n  prompt: {}\n  executions: {}\n",
                    job.id,
                    job.cron,
                    type_marker,
                    durable_marker,
                    if job.prompt.len() <= 80 {
                        job.prompt.clone()
                    } else {
                        let end = job.prompt.floor_char_boundary(80);
                        format!("{}...", &job.prompt[..end])
                    },
                    job.execution_count,
                ));
                if let Some(desc) = &job.description {
                    output.push_str(&format!("  description: {desc}\n"));
                }
            }
            Ok(ToolOutput::text(output))
        }
    }
}

#[cfg(test)]
#[path = "cron_list.test.rs"]
mod tests;
