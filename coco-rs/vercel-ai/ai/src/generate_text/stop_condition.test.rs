//! Tests for stop_condition.rs

use super::*;

#[test]
fn test_step_count_description() {
    let condition = step_count_is(3);
    assert!(condition.description().contains("3"));
}

#[test]
fn test_has_tool_call_description() {
    let condition = has_tool_call("my_tool");
    assert!(condition.description().contains("my_tool"));
}

#[test]
fn test_response_contains_description() {
    let condition = response_contains("hello");
    assert!(condition.description().contains("hello"));
}
