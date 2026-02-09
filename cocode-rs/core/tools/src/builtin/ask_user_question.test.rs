use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_ask_user_question() {
    let tool = AskUserQuestionTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "questions": [{
            "question": "Which library should we use?",
            "header": "Library",
            "options": [
                {"label": "React", "description": "Popular UI framework"},
                {"label": "Vue", "description": "Progressive framework"}
            ],
            "multiSelect": false
        }]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("Which library"));
    assert!(text.contains("React"));
}

#[tokio::test]
async fn test_ask_user_question_with_answers() {
    let tool = AskUserQuestionTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "questions": [{
            "question": "Which library?",
            "header": "Library",
            "options": [
                {"label": "React", "description": "Popular"},
                {"label": "Vue", "description": "Progressive"}
            ],
            "multiSelect": false
        }],
        "answers": {
            "Library": "React"
        }
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("React"));
}

#[tokio::test]
async fn test_ask_user_question_validation() {
    let tool = AskUserQuestionTool::new();
    let mut ctx = make_context();

    // Too few options
    let input = serde_json::json!({
        "questions": [{
            "question": "Which?",
            "header": "Q",
            "options": [{"label": "A", "description": "a"}],
            "multiSelect": false
        }]
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = AskUserQuestionTool::new();
    assert_eq!(tool.name(), "AskUserQuestion");
    assert!(!tool.is_concurrent_safe());
    assert!(!tool.is_read_only());
}
