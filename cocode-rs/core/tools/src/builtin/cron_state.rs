//! Shared state for cron scheduling tools.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::edit_strategies::truncate_str;

/// A scheduled cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job ID.
    pub id: String,
    /// Standard 5-field cron expression (minute hour day-of-month month day-of-week).
    pub cron: String,
    /// The prompt or command to execute on each trigger.
    pub prompt: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this job recurs (true) or is one-shot (false).
    /// Default: true (recurring). One-shot jobs auto-delete after first execution.
    #[serde(default = "default_recurring")]
    pub recurring: bool,
    /// Whether this job persists across sessions.
    #[serde(default)]
    pub durable: bool,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
    /// Number of times this job has executed.
    #[serde(default)]
    pub execution_count: u32,
    /// Last execution timestamp (Unix seconds), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executed_at: Option<i64>,
    /// Expiry timestamp (Unix seconds). Recurring jobs auto-expire after 3 days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Job status for completed/expired tracking.
    #[serde(default)]
    pub status: CronJobStatus,
}

fn default_recurring() -> bool {
    true
}

/// Status of a cron job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CronJobStatus {
    #[default]
    Active,
    Completed,
    Expired,
}

/// Maximum number of active cron jobs allowed.
pub const MAX_CRON_JOBS: usize = 20;

/// Auto-expiry duration for recurring jobs (3 days in seconds).
pub const RECURRING_EXPIRY_SECS: i64 = 3 * 24 * 60 * 60;

/// Shared cron job store.
pub type CronJobStore = Arc<Mutex<BTreeMap<String, CronJob>>>;

/// Create a new empty cron job store.
pub fn new_cron_store() -> CronJobStore {
    Arc::new(Mutex::new(BTreeMap::new()))
}

/// Generate a short unique cron job ID.
pub fn generate_cron_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("cron_{}", &uuid.to_string()[..8])
}

/// Serialize the full cron store to a JSON value for ContextModifier.
pub fn jobs_to_value(jobs: &BTreeMap<String, CronJob>) -> Value {
    serde_json::to_value(jobs).unwrap_or_else(|e| {
        tracing::error!("CronJob serialization failed: {e}");
        Value::Object(Default::default())
    })
}

/// Parse a schedule input: accepts either a simple interval (`5m`, `1h`, `30s`, `2d`)
/// or a standard 5-field cron expression. Returns the normalized 5-field cron expression.
pub fn parse_schedule(input: &str) -> std::result::Result<String, String> {
    let trimmed = input.trim();

    // Try simple interval format: digits followed by s/m/h/d
    if let Some(cron) = parse_simple_interval(trimmed) {
        return Ok(cron);
    }

    // Otherwise, validate as standard 5-field cron expression
    if validate_cron_expression(trimmed) {
        Ok(trimmed.to_string())
    } else {
        Err(format!(
            "Invalid schedule: '{trimmed}'. Expected a simple interval (e.g., '5m', '1h', '30s') \
             or a 5-field cron expression (minute hour day-of-month month day-of-week)."
        ))
    }
}

/// Parse a simple interval like `5m`, `1h`, `30s`, `2d` into a 5-field cron expression.
fn parse_simple_interval(input: &str) -> Option<String> {
    let re_like = input.len() >= 2
        && input[..input.len() - 1].chars().all(|c| c.is_ascii_digit())
        && matches!(input.as_bytes().last(), Some(b's' | b'm' | b'h' | b'd'));

    if !re_like {
        return None;
    }

    let value: u64 = input[..input.len() - 1].parse().ok()?;
    let unit = input.as_bytes().last()?;

    if value == 0 {
        return None;
    }

    match unit {
        b's' => {
            // Seconds: minimum granularity is 1 minute for cron, round up
            // For <60s, use every-minute; for >=60s, convert to minutes
            let mins = value.div_ceil(60);
            if mins >= 60 {
                Some("0 * * * *".to_string()) // every hour
            } else {
                Some(format!("*/{mins} * * * *"))
            }
        }
        b'm' => {
            if value >= 60 {
                // Convert to hours
                let hours = value / 60;
                if hours >= 24 {
                    Some("0 0 * * *".to_string()) // daily
                } else {
                    Some(format!("0 */{hours} * * *"))
                }
            } else {
                Some(format!("*/{value} * * * *"))
            }
        }
        b'h' => {
            if value >= 24 {
                Some("0 0 * * *".to_string()) // daily
            } else {
                Some(format!("0 */{value} * * *"))
            }
        }
        b'd' => {
            if value == 1 {
                Some("0 0 * * *".to_string()) // daily
            } else {
                Some(format!("0 0 */{value} * *"))
            }
        }
        _ => None,
    }
}

/// Validate a cron expression (standard 5-field format).
///
/// Checks: 5 space-separated fields, valid characters, and value ranges:
/// - Minute: 0-59
/// - Hour: 0-23
/// - Day of month: 1-31
/// - Month: 1-12
/// - Day of week: 0-7 (0 and 7 both represent Sunday)
pub fn validate_cron_expression(expr: &str) -> bool {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let ranges: [(u32, u32); 5] = [
        (0, 59), // minute
        (0, 23), // hour
        (1, 31), // day of month
        (1, 12), // month
        (0, 7),  // day of week (0 and 7 = Sunday)
    ];

    for (field, &(min, max)) in fields.iter().zip(ranges.iter()) {
        if !validate_cron_field(field, min, max) {
            return false;
        }
    }
    true
}

/// Validate a single cron field against its allowed range.
fn validate_cron_field(field: &str, min: u32, max: u32) -> bool {
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }

        // Handle step: */N or range/N or value/N
        let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
            match s.parse::<u32>() {
                Ok(v) if v > 0 => (r, Some(v)),
                _ => return false,
            }
        } else {
            (part, None)
        };

        if range_part == "*" || range_part == "?" {
            // Wildcard — valid. Step (if any) must fit within range.
            if let Some(s) = step
                && s > max - min + 1
            {
                return false;
            }
            continue;
        }

        // Handle range: lo-hi
        if let Some((lo_str, hi_str)) = range_part.split_once('-') {
            let lo: u32 = match lo_str.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            let hi: u32 = match hi_str.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            if lo < min || hi > max || lo > hi {
                return false;
            }
            continue;
        }

        // Exact number
        let val: u32 = match range_part.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if val < min || val > max {
            return false;
        }
    }
    true
}

/// Format cron jobs as human-readable summary.
pub fn format_cron_summary<'a>(jobs: impl Iterator<Item = &'a CronJob>) -> String {
    let mut output = String::new();
    for job in jobs {
        let type_marker = if job.recurring { "" } else { " (one-shot)" };
        let durable_marker = if job.durable { " [durable]" } else { "" };
        let status_marker = match job.status {
            CronJobStatus::Completed => " [completed]",
            CronJobStatus::Expired => " [expired]",
            CronJobStatus::Active => "",
        };
        output.push_str(&format!(
            "- {}: [{}]{}{}{}\n  prompt: {}\n  executions: {}\n",
            job.id,
            job.cron,
            type_marker,
            durable_marker,
            status_marker,
            truncate_str(&job.prompt, 80),
            job.execution_count,
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

/// File name for durable cron persistence.
const SCHEDULED_TASKS_FILE: &str = "scheduled_tasks.json";

/// Save durable cron jobs to `{cocode_home}/scheduled_tasks.json`.
///
/// Filters the store to `durable == true && status == Active`, serializes to JSON,
/// and writes atomically (write to `.tmp` then rename).
pub async fn save_durable_jobs(
    store: &CronJobStore,
    cocode_home: &std::path::Path,
) -> std::io::Result<()> {
    let guard = store.lock().await;
    let durable: BTreeMap<String, CronJob> = guard
        .iter()
        .filter(|(_, j)| j.durable && j.status == CronJobStatus::Active)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    drop(guard);

    let json = serde_json::to_string_pretty(&durable)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let target = cocode_home.join(SCHEDULED_TASKS_FILE);
    let tmp = cocode_home.join(format!("{SCHEDULED_TASKS_FILE}.tmp"));

    tokio::fs::write(&tmp, json.as_bytes()).await?;
    tokio::fs::rename(&tmp, &target).await?;
    Ok(())
}

/// Load durable cron jobs from `{cocode_home}/scheduled_tasks.json`.
///
/// Recalculates `expires_at` for jobs whose expiry passed during downtime.
/// Truly expired jobs (past `RECURRING_EXPIRY_SECS` from creation) are skipped.
pub async fn load_durable_jobs(
    cocode_home: &std::path::Path,
) -> std::io::Result<BTreeMap<String, CronJob>> {
    let path = cocode_home.join(SCHEDULED_TASKS_FILE);
    let data = tokio::fs::read_to_string(&path).await?;
    let mut jobs: BTreeMap<String, CronJob> = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Recalculate expiry for jobs that expired during downtime
    jobs.retain(|_, job| {
        if job.status != CronJobStatus::Active {
            return false;
        }
        // Check if the job is truly expired (past RECURRING_EXPIRY_SECS from creation)
        let age = now.saturating_sub(job.created_at);
        if age > RECURRING_EXPIRY_SECS {
            return false;
        }
        // Recalculate expires_at from remaining lifetime
        if job.recurring {
            let remaining = RECURRING_EXPIRY_SECS.saturating_sub(age);
            job.expires_at = Some(now + remaining);
        }
        true
    });

    Ok(jobs)
}
