use super::*;

use std::sync::Arc;

use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::LanguageModelMessage;
use coco_inference::ToolContentPart;
use coco_inference::ToolResultContent;
use coco_messages::ToolResult as CocoToolResult;
use coco_messages::ToolResultContentPart;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

struct RenderOnlyTool {
    parts: Vec<ToolResultContentPart>,
    is_mcp: bool,
    max_result_size_bound: coco_tool_runtime::ResultSizeBound,
}

#[async_trait::async_trait]
impl Tool for RenderOnlyTool {
    fn id(&self) -> ToolId {
        ToolId::Custom("RenderOnly".into())
    }

    fn name(&self) -> &str {
        "RenderOnly"
    }

    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema::default()
    }

    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "render only".into()
    }

    async fn execute(
        &self,
        _: Value,
        _: &ToolUseContext,
    ) -> Result<CocoToolResult<Value>, ToolError> {
        unreachable!("tests inject execute_result directly")
    }

    fn render_for_model(&self, _: &Value) -> Vec<ToolResultContentPart> {
        self.parts.clone()
    }

    fn max_result_size_bound(&self) -> coco_tool_runtime::ResultSizeBound {
        self.max_result_size_bound
    }

    fn is_mcp(&self) -> bool {
        self.is_mcp
    }
}

fn text_part(text: impl Into<String>) -> ToolResultContentPart {
    ToolResultContentPart::Text {
        text: text.into(),
        provider_options: None,
    }
}

fn test_orchestration_ctx() -> OrchestrationContext {
    OrchestrationContext {
        session_id: "session-1".into(),
        cwd: std::path::PathBuf::from("/tmp"),
        project_dir: None,
        permission_mode: None,
        transcript_path: None,
        agent_id: None,
        agent_type: None,
        cancel: CancellationToken::new(),
        disable_all_hooks: false,
        allow_managed_hooks_only: false,
        workspace_trust_accepted: None,
        attachment_emitter: coco_messages::AttachmentEmitter::noop(),
        sync_event_sink: None,
        http_url_allowlist: None,
        http_env_var_policy: None,
        async_registry: None,
        llm_handle: None,
    }
}

fn tool_result_text(message: &Message) -> (&str, bool) {
    let Message::ToolResult(tr) = message else {
        panic!("expected tool result");
    };
    let LanguageModelMessage::Tool { content, .. } = &tr.message else {
        panic!("expected tool-role message");
    };
    let ToolContentPart::ToolResult(result) = &content[0] else {
        panic!("expected tool result content");
    };
    let text = match &result.output {
        ToolResultContent::Text { value, .. } | ToolResultContent::ErrorText { value, .. } => {
            value.as_str()
        }
        other => panic!("expected text output, got {other:?}"),
    };
    (text, result.is_error)
}

#[tokio::test]
async fn text_only_multipart_output_uses_level1_persistence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let first = "a".repeat(30_000);
    let second = "b".repeat(30_000);
    let tool = Arc::new(RenderOnlyTool {
        parts: vec![text_part(first.clone()), text_part(second.clone())],
        is_mcp: false,
        max_result_size_bound: coco_tool_runtime::ResultSizeBound::Chars(100_000),
    });

    let outcome = build_outcome_from_execution(RunOneTail {
        tool_use_id: "call-1".into(),
        tool_id: tool.id(),
        tool_name: tool.name().into(),
        model_index: 0,
        tool,
        effective_input: Value::Null,
        execute_result: Ok(CocoToolResult::data(Value::String("ignored".into()))),
        hooks: None,
        orchestration_ctx: test_orchestration_ctx(),
        hook_tx: None,
        tool_result_session_dir: Some(tmp.path().join("session-1")),
    })
    .await;

    let (text, is_error) = tool_result_text(&outcome.ordered_messages[0]);
    assert!(!is_error);
    assert!(text.starts_with("<persisted-output>"), "got: {text}");
    let persisted = tmp.path().join("session-1/tool-results/call-1.txt");
    assert_eq!(
        std::fs::read_to_string(persisted).unwrap(),
        format!("{first}\n\n{second}")
    );
}

#[tokio::test]
async fn mcp_error_envelope_creates_error_tool_result() {
    let tool = Arc::new(RenderOnlyTool {
        parts: vec![text_part("server failed")],
        is_mcp: true,
        max_result_size_bound: coco_tool_runtime::ResultSizeBound::Chars(100_000),
    });

    let outcome = build_outcome_from_execution(RunOneTail {
        tool_use_id: "call-err".into(),
        tool_id: tool.id(),
        tool_name: tool.name().into(),
        model_index: 0,
        tool,
        effective_input: Value::Null,
        execute_result: Ok(CocoToolResult::data(serde_json::json!({
            "error": true,
            "content": [{"type": "text", "text": "server failed"}],
        }))),
        hooks: None,
        orchestration_ctx: test_orchestration_ctx(),
        hook_tx: None,
        tool_result_session_dir: None,
    })
    .await;

    let (text, is_error) = tool_result_text(&outcome.ordered_messages[0]);
    assert_eq!(text, "server failed");
    assert!(is_error);
}
