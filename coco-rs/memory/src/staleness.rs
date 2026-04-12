//! Memory staleness detection and freshness reporting.
//!
//! TS: memdir/memoryAge.ts — memoryAgeDays, memoryAge, memoryFreshnessText,
//! memoryFreshnessNote.

use std::path::Path;
use std::time::SystemTime;

/// Number of days since a file was last modified.
///
/// Returns 0 for files modified today, 1 for yesterday, etc.
pub fn memory_age_days(mtime_ms: i64) -> i64 {
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff_ms = now_ms - mtime_ms;
    if diff_ms < 0 {
        return 0;
    }
    diff_ms / (24 * 60 * 60 * 1000)
}

/// Human-readable age string for a memory file.
///
/// TS: memoryAge.ts — returns "today", "yesterday", or "{N} days ago".
pub fn memory_age(mtime_ms: i64) -> String {
    let days = memory_age_days(mtime_ms);
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        _ => format!("{days} days ago"),
    }
}

/// Freshness warning text for memories older than 1 day.
///
/// TS: memoryFreshnessText — returns empty for ≤1 day (today AND yesterday).
/// Returns `None` for fresh memories (modified today or yesterday).
pub fn memory_freshness_text(mtime_ms: i64) -> Option<String> {
    let days = memory_age_days(mtime_ms);
    if days <= 1 {
        return None;
    }
    let age = memory_age(mtime_ms);
    Some(format!(
        "This memory was last updated {age}. \
         Its content may be outdated — verify against current state before acting on it."
    ))
}

/// Freshness note wrapped in `<system-reminder>` XML tags.
///
/// Injected alongside relevant memory attachments for memories older than 1 day.
pub fn memory_freshness_note(mtime_ms: i64) -> Option<String> {
    memory_freshness_text(mtime_ms).map(|text| format!("<system-reminder>{text}</system-reminder>"))
}

/// The drift caveat used inline in the "When to access memories" section.
///
/// TS: memoryTypes.ts MEMORY_DRIFT_CAVEAT — single bullet point.
pub const MEMORY_DRIFT_CAVEAT: &str = "\
Memory records can become stale over time. Use memory as context for what was true at a \
given point in time. Before answering the user or building assumptions based solely on \
information in memory records, verify that the memory is still correct and up-to-date by \
reading the current state of the files or resources. If a recalled memory conflicts with \
current information, trust what you observe now — and update or remove the stale memory \
rather than acting on it.";

/// Get the mtime of a file in milliseconds since epoch.
pub fn file_mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Staleness info for a memory entry.
#[derive(Debug, Clone)]
pub struct StalenessInfo {
    /// Age in days since last modification.
    pub age_days: i64,
    /// Human-readable age string.
    pub age_text: String,
    /// Whether the memory is considered stale (>1 day old, i.e. not today/yesterday).
    pub is_stale: bool,
    /// Warning text for stale memories.
    pub warning: Option<String>,
}

impl StalenessInfo {
    /// Compute staleness info from a file's mtime in milliseconds.
    pub fn from_mtime_ms(mtime_ms: i64) -> Self {
        let age_days = memory_age_days(mtime_ms);
        let age_text = memory_age(mtime_ms);
        let is_stale = age_days > 1;
        let warning = memory_freshness_text(mtime_ms);
        Self {
            age_days,
            age_text,
            is_stale,
            warning,
        }
    }
}

#[cfg(test)]
#[path = "staleness.test.rs"]
mod tests;
