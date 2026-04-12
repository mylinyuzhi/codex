use super::*;

#[test]
fn test_truncate_tool_description() {
    let short = "A short description";
    assert_eq!(truncate_tool_description(short), short);

    let long = "x".repeat(3000);
    let truncated = truncate_tool_description(&long);
    assert!(truncated.len() < 3000);
    assert!(truncated.ends_with("...(truncated)"));
}
