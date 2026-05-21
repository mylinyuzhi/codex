use coco_messages::ToolResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
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
            required: Vec::new(),
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
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

#[test]
fn test_tool_default_flags() {
    let tool = EchoTool;
    let ctx = crate::context::ToolUseContext::test_default();
    assert!(tool.is_enabled(&ctx));
    assert!(tool.is_read_only(&json!({})));
    assert!(tool.is_concurrency_safe(&json!({})));
    assert!(!tool.is_destructive(&json!({})));
    assert!(!tool.should_defer());
    assert!(!tool.always_load());
    assert_eq!(tool.interrupt_behavior(), InterruptBehavior::Block);
    // TS default declaration is 100K; persistence clamps it to the
    // 50K storage cap. Canonical tools opt out by overriding to
    // `ResultSizeBound::Unbounded`.
    assert_eq!(
        tool.max_result_size_bound(),
        crate::tool_result_storage::DEFAULT_TOOL_MAX_RESULT_SIZE_BOUND
    );
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
                required: Vec::new(),
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
        ) -> Result<coco_messages::ToolResult<serde_json::Value>, super::super::ToolError> {
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
        skill_names: vec![],
        ..Default::default()
    };
    let desc_opts = prompt_opts.as_description_options();
    assert!(desc_opts.is_non_interactive);
    assert_eq!(desc_opts.tool_names.len(), 2);
}

/// Byte-identity contract for the default `Tool::render_for_model`
/// impl. Critical regression guard: the default impl MUST produce
/// the same string as the pre-refactor `serde_json::to_string(&data)`
/// path — this is what `app/query/src/tool_outcome_builder.rs`
/// detects via the singleton-Text fast path. If this drifts, every
/// non-overriding tool's wire output silently changes.
#[test]
fn render_for_model_default_is_singleton_json_text() {
    let tool = EchoTool;
    let data = json!({"foo": 42, "bar": ["a", "b"]});
    let parts = tool.render_for_model(&data);

    let expected = serde_json::to_string(&data).unwrap();
    assert_eq!(
        parts,
        vec![ToolResultContentPart::Text {
            text: expected,
            provider_options: None,
        }]
    );
}

#[test]
fn render_for_model_default_handles_null_data() {
    let tool = EchoTool;
    let parts = tool.render_for_model(&Value::Null);
    assert_eq!(
        parts,
        vec![ToolResultContentPart::Text {
            text: "null".into(),
            provider_options: None,
        }]
    );
}

/// `render_text_or_json` unwraps a bare `Value::String` directly
/// (no JSON quotes around the content). Used by tools whose
/// `execute()` already builds the human-readable string.
#[test]
fn render_text_or_json_unwraps_bare_string() {
    let parts = render_text_or_json(&Value::String("plain text\nwith newline".into()));
    assert_eq!(
        parts,
        vec![ToolResultContentPart::Text {
            text: "plain text\nwith newline".into(),
            provider_options: None,
        }]
    );
}

/// Non-string `data` falls back to JSON-stringify — same shape as
/// the trait's default impl. Guards against passing structured data
/// to a tool that accidentally uses the helper without a custom branch.
#[test]
fn render_text_or_json_falls_back_to_json_for_structured_data() {
    let data = json!({"id": 7, "ok": true});
    let parts = render_text_or_json(&data);
    assert_eq!(
        parts,
        vec![ToolResultContentPart::Text {
            text: serde_json::to_string(&data).unwrap(),
            provider_options: None,
        }]
    );
}
