use super::*;

#[test]
fn test_format_elapsed() {
    assert_eq!(format_elapsed(5_000), "5s");
    assert_eq!(format_elapsed(65_000), "1:05");
    assert_eq!(format_elapsed(3_600_000), "60:00");
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(2500), "2.5k");
}
