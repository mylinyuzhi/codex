use super::*;
use std::path::PathBuf;

#[tokio::test]
async fn test_exit_plan_mode() {
    let tool = ExitPlanModeTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Should return structured content with plan info
    if let cocode_protocol::ToolResultContent::Structured(content) = &result.content {
        assert!(content.get("plan").is_some());
        assert!(content.get("isAgent").is_some());
        assert!(content.get("filePath").is_some());
    }
}

#[tokio::test]
async fn test_exit_plan_mode_with_prompts() {
    let tool = ExitPlanModeTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({
        "allowedPrompts": [
            {"tool": "Bash", "prompt": "run tests"},
            {"tool": "Bash", "prompt": "install dependencies"}
        ]
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_exit_plan_mode_as_agent() {
    let tool = ExitPlanModeTool::new();
    let mut ctx =
        ToolContext::new("call-1", "session-1", PathBuf::from("/tmp")).with_agent_id("explore-1");

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // isAgent should be true
    if let cocode_protocol::ToolResultContent::Structured(content) = &result.content {
        assert_eq!(content.get("isAgent"), Some(&serde_json::json!(true)));
    }
}

#[test]
fn test_tool_properties() {
    let tool = ExitPlanModeTool::new();
    assert_eq!(tool.name(), "ExitPlanMode");
    assert!(!tool.is_concurrent_safe());
}
