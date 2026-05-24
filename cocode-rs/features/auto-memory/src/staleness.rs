//! Memory staleness detection.
//!
//! Tracks how old a memory file is and generates warnings when
//! the content may be outdated.

use std::time::Duration;
use std::time::SystemTime;

/// Staleness information for a memory file.
#[derive(Debug, Clone)]
pub struct StalenessInfo {
    /// Days since the file was last modified.
    pub days_since_modified: i64,
    /// Human-readable relative time (e.g., "today", "2 days ago").
    pub relative_time: String,
    /// Whether a staleness warning should be shown (>1 day old).
    pub needs_warning: bool,
    /// Warning text (empty if fresh).
    pub warning: String,
}

/// Build staleness info from a file's last-modified timestamp.
pub fn staleness_info(last_modified: SystemTime, warning_threshold_days: i64) -> StalenessInfo {
    let days = get_days_since(last_modified);
    let relative_time = format_relative_time(days);
    let warning = build_staleness_warning(days, warning_threshold_days);
    let needs_warning = !warning.is_empty();

    StalenessInfo {
        days_since_modified: days,
        relative_time,
        needs_warning,
        warning,
    }
}

/// Format a relative time string from days since modification.
pub fn format_relative_time(days: i64) -> String {
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        d => format!("{d} days ago"),
    }
}

/// Build a staleness warning for memory files older than the threshold.
///
/// Returns an empty string for memories within the threshold.
pub fn build_staleness_warning(days: i64, threshold_days: i64) -> String {
    if days <= threshold_days {
        return String::new();
    }

    format!(
        "This memory is {days} days old. \
         Memories are point-in-time observations, not live state — \
         claims about code behavior or file:line citations may be outdated. \
         Verify against current code before asserting as fact."
    )
}

/// Calculate days since a timestamp.
fn get_days_since(timestamp: SystemTime) -> i64 {
    let elapsed = timestamp.elapsed().unwrap_or(Duration::ZERO).as_secs();
    (elapsed / 86400) as i64
}

#[cfg(test)]
#[path = "staleness.test.rs"]
mod tests;
