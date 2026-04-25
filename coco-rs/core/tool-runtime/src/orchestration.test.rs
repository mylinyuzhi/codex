use super::*;

#[test]
fn test_truncate_within_budget() {
    let short = "hello world";
    assert_eq!(truncate_tool_result(short), short);
}

#[test]
fn test_truncate_over_budget() {
    let long = "x".repeat(200_000);
    let truncated = truncate_tool_result(&long);
    assert!(truncated.len() < 200_000);
    assert!(truncated.contains("[output truncated"));
}
