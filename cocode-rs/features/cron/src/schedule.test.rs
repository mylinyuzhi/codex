use super::*;

#[test]
fn test_parse_schedule_simple_minutes() {
    assert_eq!(parse_schedule("5m").unwrap(), "*/5 * * * *");
    assert_eq!(parse_schedule("10m").unwrap(), "*/10 * * * *");
    assert_eq!(parse_schedule("1m").unwrap(), "*/1 * * * *");
}

#[test]
fn test_parse_schedule_simple_hours() {
    assert_eq!(parse_schedule("1h").unwrap(), "0 */1 * * *");
    assert_eq!(parse_schedule("2h").unwrap(), "0 */2 * * *");
}

#[test]
fn test_parse_schedule_simple_seconds() {
    assert_eq!(parse_schedule("30s").unwrap(), "*/1 * * * *");
    assert_eq!(parse_schedule("90s").unwrap(), "*/2 * * * *");
}

#[test]
fn test_parse_schedule_simple_days() {
    assert_eq!(parse_schedule("1d").unwrap(), "0 0 * * *");
    assert_eq!(parse_schedule("3d").unwrap(), "0 0 */3 * *");
}

#[test]
fn test_parse_schedule_zero_value() {
    assert!(parse_schedule("0m").is_err());
}

#[test]
fn test_parse_schedule_cron_expression() {
    assert_eq!(parse_schedule("*/5 * * * *").unwrap(), "*/5 * * * *");
    assert_eq!(parse_schedule("0 9 * * 1-5").unwrap(), "0 9 * * 1-5");
}

#[test]
fn test_parse_schedule_invalid() {
    assert!(parse_schedule("invalid").is_err());
    assert!(parse_schedule("* * *").is_err());
    assert!(parse_schedule("60 * * * *").is_err());
}

#[test]
fn test_validate_cron_expression_valid() {
    assert!(validate_cron_expression("* * * * *"));
    assert!(validate_cron_expression("*/5 * * * *"));
    assert!(validate_cron_expression("0 9 * * 1-5"));
    assert!(validate_cron_expression("0,15,30,45 * * * *"));
    assert!(validate_cron_expression("0-30/10 * * * *"));
}

#[test]
fn test_validate_cron_expression_invalid() {
    assert!(!validate_cron_expression("* * * *")); // 4 fields
    assert!(!validate_cron_expression("60 * * * *")); // minute > 59
    assert!(!validate_cron_expression("* 24 * * *")); // hour > 23
    assert!(!validate_cron_expression("* * 0 * *")); // day < 1
    assert!(!validate_cron_expression("* * * 13 *")); // month > 12
    assert!(!validate_cron_expression("*/0 * * * *")); // step of 0
}

#[test]
fn test_validate_cron_field_range() {
    assert!(validate_cron_field("1-5", 0, 59));
    assert!(!validate_cron_field("5-3", 0, 59)); // inverted range
    assert!(!validate_cron_field("0-60", 0, 59)); // hi > max
}

#[test]
fn test_validate_cron_field_step() {
    assert!(validate_cron_field("*/10", 0, 59));
    assert!(!validate_cron_field("*/61", 0, 59)); // step > range
}

#[test]
fn test_parse_schedule_large_minutes_to_hours() {
    assert_eq!(parse_schedule("120m").unwrap(), "0 */2 * * *");
}

#[test]
fn test_parse_schedule_large_hours_to_daily() {
    assert_eq!(parse_schedule("48h").unwrap(), "0 0 * * *");
}

#[test]
fn test_validate_day_of_week_7() {
    // 7 is valid (represents Sunday, same as 0)
    assert!(validate_cron_expression("0 0 * * 7"));
}
