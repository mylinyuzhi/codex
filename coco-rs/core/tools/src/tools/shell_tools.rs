use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

// ── SleepTool ──

/// Typed input for [`SleepTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SleepInput {
    /// Number of seconds to sleep. Defaults to `1.0` when omitted.
    /// Capped at 300 seconds (5 minutes) to prevent indefinite
    /// blocking.
    #[serde(default)]
    pub seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SleepOutput {
    /// Human-readable confirmation message.
    pub message: String,
    /// Actual seconds slept (post-cap).
    pub seconds: f64,
}

pub struct SleepTool;

#[async_trait::async_trait]
impl Tool for SleepTool {
    type Input = SleepInput;
    type Output = SleepOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Sleep)
    }
    fn name(&self) -> &str {
        ToolName::Sleep.as_str()
    }
    fn description(&self, _input: &SleepInput, _options: &DescriptionOptions) -> String {
        "Sleep for a specified number of seconds.".into()
    }
    fn is_read_only(&self, _input: &SleepInput) -> bool {
        true
    }
    /// Pure time-passing — Plan mode keeps Sleep visible.
    fn is_always_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _input: &SleepInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("pause execution for a configurable number of seconds")
    }

    fn render_for_model(&self, out: &SleepOutput) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.message.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: SleepInput,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<SleepOutput>, ToolError> {
        let seconds = input.seconds.unwrap_or(1.0);

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
            data: SleepOutput {
                message: format!("Slept for {capped:.1} seconds"),
                seconds: capped,
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
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

/// Typed input for [`ReplTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReplInput {
    /// Programming language for the REPL (e.g., `python`, `node`).
    #[serde(default)]
    pub language: Option<String>,
    /// Command to execute in the REPL.
    #[serde(default)]
    pub command: Option<String>,
}

/// REPL output. Currently unused — the tool errors out at the
/// `execute` boundary. Kept typed so a future REPL backend can
/// produce structured output (stdout / language metadata / continuation
/// state) without breaking the trait.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ReplOutput {
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

pub struct ReplTool;

#[async_trait::async_trait]
impl Tool for ReplTool {
    type Input = ReplInput;
    type Output = ReplOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Repl)
    }
    fn name(&self) -> &str {
        ToolName::Repl.as_str()
    }
    fn description(&self, _input: &ReplInput, _options: &DescriptionOptions) -> String {
        "Start an interactive REPL session for a supported language.".into()
    }

    fn is_transparent_wrapper(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: ReplInput,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<ReplOutput>, ToolError> {
        Err(ToolError::ExecutionFailed {
            message: "REPL tool is not available in this context. \
                      Use the Bash tool to run language-specific commands instead \
                      (e.g., `python3 -c \"...\"` or `node -e \"...\"`)."
                .into(),
            source: None,
        })
    }
}

// `StructuredOutputTool` lives in `tools/structured_output.rs` because
// it needs a compiled `jsonschema::Validator` at construction time and
// is conditionally registered (only in non-interactive sessions when
// `--json-schema` is supplied) — it doesn't ride the same always-on
// registration path as the rest of this module.
pub use super::structured_output::StructuredOutputTool;
