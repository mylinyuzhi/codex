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
        let summary = if include_completed {
            cron_state::format_cron_summary(store.values())
        } else {
            cron_state::format_cron_summary(
                store
                    .values()
                    .filter(|j| j.status == cron_state::CronJobStatus::Active),
            )
        };
        Ok(ToolOutput::text(summary))
    }
}

#[cfg(test)]
#[path = "cron_list.test.rs"]
mod tests;
