use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── SleepTool ──

pub struct SleepTool;

#[async_trait::async_trait]
impl Tool for SleepTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Sleep)
    }
    fn name(&self) -> &str {
        ToolName::Sleep.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Sleep for a specified number of seconds.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "seconds".into(),
            serde_json::json!({"type": "number", "description": "Number of seconds to sleep"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let seconds = input
            .get("seconds")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);

        if seconds < 0.0 {
            return Err(ToolError::InvalidInput {
                message: "seconds must be non-negative".into(),
                error_code: None,
            });
        }

        // Cap at 5 minutes to prevent indefinite blocking
        let capped = seconds.min(300.0);
        let duration = std::time::Duration::from_secs_f64(capped);
        tokio::time::sleep(duration).await;

        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Slept for {capped:.1} seconds"),
                "seconds": capped,
            }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// PowerShellTool lives in `powershell_tool.rs` so the security pipeline
// (`analyze_ps_security`, `find_unsafe_type_references`, `decode_ps_output`
// in `powershell.rs`) can be wired around the pwsh subprocess. The
// support utilities existed before but weren't called from the execute
// path — the dedicated module closes that gap.
pub use super::powershell_tool::PowerShellTool;

// ── ReplTool ──

pub struct ReplTool;

#[async_trait::async_trait]
impl Tool for ReplTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Repl)
    }
    fn name(&self) -> &str {
        ToolName::Repl.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Start an interactive REPL session for a supported language.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "language".into(),
            serde_json::json!({"type": "string", "description": "Programming language for the REPL (e.g., python, node)"}),
        );
        p.insert(
            "command".into(),
            serde_json::json!({"type": "string", "description": "Command to execute in the REPL"}),
        );
        ToolInputSchema { properties: p }
    }

    fn is_transparent_wrapper(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        Err(ToolError::ExecutionFailed {
            message: "REPL tool is not available in this context. \
                      Use the Bash tool to run language-specific commands instead \
                      (e.g., `python3 -c \"...\"` or `node -e \"...\"`)."
                .into(),
            source: None,
        })
    }
}

// ── SyntheticOutputTool ──

pub struct SyntheticOutputTool;

#[async_trait::async_trait]
impl Tool for SyntheticOutputTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::SyntheticOutput)
    }
    fn name(&self) -> &str {
        ToolName::SyntheticOutput.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Emit synthetic output for SDK integrations. Returns the provided output text directly."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "output".into(),
            serde_json::json!({"type": "string", "description": "Output text to emit"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let output = input.get("output").and_then(|v| v.as_str()).unwrap_or("");

        Ok(ToolResult {
            data: serde_json::json!(output),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}
