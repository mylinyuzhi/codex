use super::*;
use std::path::PathBuf;

#[tokio::test]
async fn test_enter_plan_mode_default() {
    let tool = EnterPlanModeTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Should return structured content with plan info
    if let cocode_protocol::ToolResultContent::Structured(content) = &result.content {
        assert!(content.get("planFilePath").is_some());
        assert!(content.get("slug").is_some());
        assert!(content.get("message").is_some());
        // Non-interview message should contain step-by-step guide
        let message = content["message"].as_str().unwrap();
        assert!(message.contains("Entered plan mode"));
        assert!(message.contains(cocode_protocol::ToolName::ExitPlanMode.as_str()));
    } else {
        panic!("Expected Structured output, got Text");
    }
}

#[tokio::test]
async fn test_enter_plan_mode_interview_phase() {
    let tool = EnterPlanModeTool::with_interview_phase(true);
    let mut features = cocode_protocol::Features::with_defaults();
    features.enable(cocode_protocol::Feature::PlanModeInterview);
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));
    ctx.features = features;

    let input = serde_json::json!({});
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    if let cocode_protocol::ToolResultContent::Structured(content) = &result.content {
        let message = content["message"].as_str().unwrap();
        // Interview message should be brief placeholder
        assert!(message.contains("DO NOT write a plan yet"));
        assert!(message.contains("detailed instructions will follow"));
    } else {
        panic!("Expected Structured output, got Text");
    }
}

#[test]
fn test_tool_properties() {
    let tool = EnterPlanModeTool::new();
    assert_eq!(
        tool.name(),
        cocode_protocol::ToolName::EnterPlanMode.as_str()
    );
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only());
}

#[test]
fn test_description_varies_by_interview_phase() {
    let tool_default = EnterPlanModeTool::new();
    let tool_interview = EnterPlanModeTool::with_interview_phase(true);

    // Default description includes workflow section
    assert!(tool_default.description().contains("In plan mode"));

    // Interview description omits workflow section
    assert!(!tool_interview.description().contains("In plan mode"));

    // Both should include "When to use" section
    assert!(tool_default.description().contains("When to use"));
    assert!(tool_interview.description().contains("When to use"));
}
