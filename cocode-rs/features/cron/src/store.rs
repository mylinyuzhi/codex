//! Shared cron job store with formatting helpers.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::types::CronJob;
use crate::types::CronJobStatus;

/// Thread-safe shared cron job store.
pub type CronJobStore = Arc<Mutex<BTreeMap<String, CronJob>>>;

/// Create a new empty cron job store.
pub fn new_cron_store() -> CronJobStore {
    Arc::new(Mutex::new(BTreeMap::new()))
}

/// Serialize the full cron store to a JSON value for ContextModifier.
pub fn jobs_to_value(jobs: &BTreeMap<String, CronJob>) -> Value {
    serde_json::to_value(jobs).unwrap_or_else(|e| {
        tracing::error!("CronJob serialization failed: {e}");
        Value::Object(Default::default())
    })
}

/// Format cron jobs as a human-readable summary.
pub fn format_cron_summary<'a>(jobs: impl Iterator<Item = &'a CronJob>) -> String {
    let mut output = String::new();
    for job in jobs {
        let type_marker = if job.recurring { "" } else { " (one-shot)" };
        let durable_marker = if job.durable { " [durable]" } else { "" };
        let status_marker = match job.status {
            CronJobStatus::Completed => " [completed]",
            CronJobStatus::Expired => " [expired]",
            CronJobStatus::Disabled => " [disabled]",
            CronJobStatus::Active => "",
        };

        let prompt_display = cocode_utils_string::truncate_str(&job.prompt, 80);
        output.push_str(&format!(
            "- {}: [{}]{}{}{}\n  prompt: {prompt_display}\n  executions: {}\n",
            job.id, job.cron, type_marker, durable_marker, status_marker, job.execution_count,
        ));
        if let Some(desc) = &job.description {
            output.push_str(&format!("  description: {desc}\n"));
        }
    }
    if output.is_empty() {
        return "No scheduled jobs.".to_string();
    }
    output
}

#[cfg(test)]
#[path = "store.test.rs"]
mod tests;
