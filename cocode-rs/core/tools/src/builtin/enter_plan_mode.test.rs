use super::*;
use std::path::PathBuf;

#[tokio::test]
async fn test_enter_plan_mode() {
    let tool = EnterPlanModeTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Check output contains plan file path
    if let cocode_protocol::ToolResultContent::Text(content) = &result.content {
        assert!(content.contains("Plan file:"));
        assert!(content.contains("Write tool"));
        assert!(content.contains("Edit tool"));
    }
}

#[tokio::test]
async fn test_enter_plan_mode_with_agent_id() {
    let tool = EnterPlanModeTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
        .with_agent_id("explore-1");

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[test]
fn test_tool_properties() {
    let tool = EnterPlanModeTool::new();
    assert_eq!(tool.name(), "EnterPlanMode");
    assert!(!tool.is_concurrent_safe());
}
