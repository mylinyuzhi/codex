//! Cron expression parsing and validation.
//!
//! Supports both simple interval format (`5m`, `1h`, `30s`, `2d`) and
//! standard 5-field cron expressions.

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
            // Seconds: minimum granularity is 1 minute for cron, round up.
            // For <60s, use every-minute; for >=60s, convert to minutes.
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

#[cfg(test)]
#[path = "schedule.test.rs"]
mod tests;
