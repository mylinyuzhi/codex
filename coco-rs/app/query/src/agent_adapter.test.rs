use coco_tool::AgentQueryConfig;
use coco_tool::AgentQueryEngine;

#[tokio::test]
async fn test_no_op_engine_returns_error() {
    let engine = coco_tool::NoOpAgentQueryEngine;
    let config = AgentQueryConfig {
        system_prompt: "test".into(),
        model: "test-model".into(),
        max_turns: Some(1),
        context_window: None,
        max_output_tokens: None,
        allowed_tools: Vec::new(),
        preserve_tool_use_results: false,
    };
    let result = engine.execute_query("hello", config).await;
    assert!(result.is_err());
}
