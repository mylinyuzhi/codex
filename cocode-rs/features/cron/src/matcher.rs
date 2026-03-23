//! Cron time matching engine.
//!
//! Supports: `*`, exact numbers, comma-separated lists, ranges (`1-5`),
//! and step values (`*/10`, `1-30/5`).

use chrono::Datelike;
use chrono::Timelike;

/// Check whether a 5-field cron expression matches the given time.
///
/// Fields: minute hour day-of-month month day-of-week
pub fn matches_cron<Tz: chrono::TimeZone>(schedule: &str, now: &chrono::DateTime<Tz>) -> bool {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let minute = now.minute();
    let hour = now.hour();
    let day = now.day();
    let month = now.month();
    // chrono: Monday=1 .. Sunday=7; cron: Sunday=0, Monday=1 .. Saturday=6
    let weekday = now.weekday().num_days_from_sunday();

    field_matches(fields[0], minute, 0, 59)
        && field_matches(fields[1], hour, 0, 23)
        && field_matches(fields[2], day, 1, 31)
        && field_matches(fields[3], month, 1, 12)
        && field_matches(fields[4], weekday, 0, 6)
}

/// Check whether a single cron field matches the given value.
///
/// Handles: `*`, `*/step`, `num`, `min-max`, `min-max/step`, and
/// comma-separated combinations of the above.
fn field_matches(field: &str, value: u32, min: u32, max: u32) -> bool {
    for part in field.split(',') {
        if part_matches(part.trim(), value, min, max) {
            return true;
        }
    }
    false
}

/// Match a single comma-element of a cron field.
fn part_matches(part: &str, value: u32, min: u32, max: u32) -> bool {
    // Split on '/' for step values.
    let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
        let step_val: u32 = match s.parse() {
            Ok(v) if v > 0 => v,
            _ => return false,
        };
        (r, Some(step_val))
    } else {
        (part, None)
    };

    // Determine the range of values this part covers.
    let (range_min, range_max) = if range_part == "*" || range_part == "?" {
        (min, max)
    } else if let Some((lo, hi)) = range_part.split_once('-') {
        let lo_val: u32 = match lo.parse() {
            Ok(v) => v,
            _ => return false,
        };
        let hi_val: u32 = match hi.parse() {
            Ok(v) => v,
            _ => return false,
        };
        (lo_val, hi_val)
    } else {
        // Exact number.
        let exact: u32 = match range_part.parse() {
            Ok(v) => v,
            _ => return false,
        };
        if let Some(s) = step {
            // e.g. "5/10" means starting at 5, every 10
            return value >= exact && (value - exact).is_multiple_of(s);
        }
        return value == exact;
    };

    // Check value is within range.
    if value < range_min || value > range_max {
        return false;
    }

    // Apply step if present.
    match step {
        Some(s) => (value - range_min).is_multiple_of(s),
        None => true,
    }
}

#[cfg(test)]
#[path = "matcher.test.rs"]
mod tests;
