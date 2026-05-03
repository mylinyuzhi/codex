//! LspTool — code-intelligence queries (definitions, references, diagnostics, symbols, hover).
//!
//! TS: `tools/LSPTool/LSPTool.ts`. Gated behind the `Lsp` feature flag.
//! When no LSP server is connected, every action returns a structured
//! error so the model gets actionable feedback rather than a silent
//! empty result.
//!
//! Note: lower-level LSP types and formatters live in `lsp.rs` —
//! pre-built scaffolding for when LSP integration is wired into the
//! tool execute path. The current execute returns a not-connected
//! stub so the tool surface matches TS even before integration lands.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;
use std::collections::HashMap;

pub struct LspTool;

#[async_trait::async_trait]
impl Tool for LspTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Lsp)
    }
    fn name(&self) -> &str {
        ToolName::Lsp.as_str()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Lsp)
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Query the Language Server Protocol for code intelligence (definitions, references, diagnostics, symbols, hover).".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "action".into(),
            serde_json::json!({"type": "string", "enum": ["definition", "references", "diagnostics", "symbols", "hover"], "description": "LSP action to perform"}),
        );
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "File path for the query"}),
        );
        p.insert(
            "symbol".into(),
            serde_json::json!({"type": "string", "description": "Symbol name to query"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_lsp(&self) -> bool {
        true
    }
    /// TS `LSPTool.ts`: `isConcurrencySafe() { return true }`. LSP queries
    /// are side-effect-free and safe to issue in parallel — the LSP server
    /// itself handles concurrent requests.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Err(ToolError::ExecutionFailed {
            message: format!(
                "LSP server is not connected. Cannot perform '{action}' action. \
                 Ensure a language server is running and configured for the current project."
            ),
            source: None,
        })
    }
}
