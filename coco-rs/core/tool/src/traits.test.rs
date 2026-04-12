use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

use super::*;

/// Minimal test tool for unit tests.
struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Read)
    }

    fn name(&self) -> &str {
        "Echo"
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        "Echoes input back".into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        Ok(ToolResult {
            data: input,
            new_messages: vec![],
        })
    }
}

#[test]
fn test_tool_default_flags() {
    let tool = EchoTool;
    assert!(tool.is_enabled());
    assert!(tool.is_read_only(&json!({})));
    assert!(tool.is_concurrency_safe(&json!({})));
    assert!(!tool.is_destructive(&json!({})));
    assert!(!tool.should_defer());
    assert!(!tool.always_load());
    assert_eq!(tool.interrupt_behavior(), InterruptBehavior::Block);
    assert_eq!(tool.max_result_size_chars(), 100_000);
    assert!(tool.mcp_info().is_none());
}

#[test]
fn test_validation_result_default_valid() {
    let tool = EchoTool;
    let ctx = ToolUseContext::test_default();
    let result = tool.validate_input(&json!({"text": "hello"}), &ctx);
    assert!(result.is_valid());
}

#[test]
fn test_backfill_default_noop() {
    let tool = EchoTool;
    let mut input = json!({"key": "value"});
    tool.backfill_observable_input(&mut input);
    assert_eq!(input, json!({"key": "value"}));
}

#[test]
fn test_prompt_options_to_description_options() {
    let prompt_opts = PromptOptions {
        is_non_interactive: true,
        tool_names: vec!["Read".into(), "Write".into()],
        agent_names: vec!["Explore".into()],
        allowed_agent_types: None,
        permission_context: None,
    };
    let desc_opts = prompt_opts.as_description_options();
    assert!(desc_opts.is_non_interactive);
    assert_eq!(desc_opts.tool_names.len(), 2);
}
