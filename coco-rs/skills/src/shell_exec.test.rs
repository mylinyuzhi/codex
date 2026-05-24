use super::*;

#[tokio::test]
async fn test_execute_shell_skip() {
    let result = execute_shell_in_prompt("Run $(echo hello)", true).await;
    assert_eq!(result, "Run $(echo hello)");
}

#[tokio::test]
async fn test_execute_shell_inline() {
    let result = execute_shell_in_prompt("Value: $(echo test_value)", false).await;
    assert_eq!(result, "Value: test_value");
}

#[tokio::test]
async fn test_execute_shell_no_patterns() {
    let result = execute_shell_in_prompt("No shell here", false).await;
    assert_eq!(result, "No shell here");
}

#[tokio::test]
async fn test_execute_shell_nested_parens() {
    let result = execute_shell_in_prompt("$(echo $(echo nested))", false).await;
    // Inner $(echo nested) is executed by shell, not by us
    assert_eq!(result, "nested");
}
