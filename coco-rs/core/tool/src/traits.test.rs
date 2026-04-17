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
    // R4: `is_open_world` defaults to false — tools are closed-world
    // unless they opt in. Matches TS `Tool.ts:434` (optional field,
    // undefined by default, no tool implements it unless it's an MCP
    // wrapper forwarding the annotation from an MCP server).
    assert!(!tool.is_open_world(&json!({})));
    // T3: `is_mcp` derives from `mcp_info().is_some()`. Built-in tools
    // (no mcp_info) are not MCP tools. Matches TS `Tool.ts:436`
    // `isMcp?: boolean` field semantics.
    assert!(!tool.is_mcp());
}

/// T3: `is_mcp` must return true for tools that advertise McpToolInfo,
/// mirroring TS `Tool.ts:436` `isMcp?: boolean` + the MCP wrapper path
/// that sets `mcpInfo` on dynamically-registered MCP tools.
#[test]
fn test_is_mcp_derives_from_mcp_info() {
    struct McpStub;
    #[async_trait::async_trait]
    impl Tool for McpStub {
        fn id(&self) -> coco_types::ToolId {
            coco_types::ToolId::Custom("mcp__server__tool".into())
        }
        fn name(&self) -> &str {
            "mcp__server__tool"
        }
        fn description(&self, _: &serde_json::Value, _: &DescriptionOptions) -> String {
            "stub".into()
        }
        fn input_schema(&self) -> coco_types::ToolInputSchema {
            coco_types::ToolInputSchema {
                properties: std::collections::HashMap::new(),
            }
        }
        fn mcp_info(&self) -> Option<&super::super::McpToolInfo> {
            static INFO: std::sync::LazyLock<super::super::McpToolInfo> =
                std::sync::LazyLock::new(|| super::super::McpToolInfo {
                    server_name: "server".into(),
                    tool_name: "tool".into(),
                });
            Some(&INFO)
        }
        async fn execute(
            &self,
            _: serde_json::Value,
            _: &ToolUseContext,
        ) -> Result<coco_types::ToolResult<serde_json::Value>, super::super::ToolError> {
            unimplemented!()
        }
    }
    assert!(
        McpStub.is_mcp(),
        "tool with mcp_info() set must return is_mcp() = true"
    );
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
