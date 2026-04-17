use coco_types::Message;
use coco_types::PermissionDecision;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolResult;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::context::ToolUseContext;
use crate::error::ToolError;
use crate::validation::ValidationResult;

/// Info about whether a tool use is a search or read operation for UI collapse.
///
/// TS: `isSearchOrReadCommand?(input)` return type.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchReadInfo {
    /// True for search operations (grep, find, glob patterns).
    pub is_search: bool,
    /// True for read operations (cat, head, tail, file read).
    pub is_read: bool,
    /// True for directory-listing operations (ls, tree, du).
    pub is_list: bool,
}

/// How a tool behaves when the user interrupts (sends new input mid-execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterruptBehavior {
    /// Cancel the tool execution immediately.
    Cancel,
    /// Block: wait for tool to finish before processing new input.
    #[default]
    Block,
}

/// Options for generating tool descriptions.
///
/// TS: `description(input, { isNonInteractiveSession, toolPermissionContext, tools })`
#[derive(Debug, Clone, Default)]
pub struct DescriptionOptions {
    /// Whether this is a non-interactive (SDK/headless) session.
    pub is_non_interactive: bool,
    /// Names of all available tools (for cross-referencing in descriptions).
    pub tool_names: Vec<String>,
    /// Permission context for tailoring descriptions to the current mode.
    /// TS: `toolPermissionContext` — tools may describe themselves differently
    /// based on what permissions are available.
    pub permission_context: Option<coco_types::ToolPermissionContext>,
}

/// Options for generating tool prompt text.
///
/// TS: `prompt({ getToolPermissionContext, tools, agents, allowedAgentTypes })`
#[derive(Debug, Clone, Default)]
pub struct PromptOptions {
    /// Whether this is a non-interactive session.
    pub is_non_interactive: bool,
    /// Names of all available tools.
    pub tool_names: Vec<String>,
    /// Available agent type names.
    pub agent_names: Vec<String>,
    /// Allowed agent types (if restricted).
    pub allowed_agent_types: Option<Vec<String>>,
    /// Permission context for tailoring prompt to current mode.
    /// TS: `getToolPermissionContext()` — async in TS, pre-resolved here.
    pub permission_context: Option<coco_types::ToolPermissionContext>,
}

impl PromptOptions {
    /// Convert to DescriptionOptions for default prompt() implementation.
    pub fn as_description_options(&self) -> DescriptionOptions {
        DescriptionOptions {
            is_non_interactive: self.is_non_interactive,
            tool_names: self.tool_names.clone(),
            permission_context: self.permission_context.clone(),
        }
    }
}

/// MCP server tool metadata.
///
/// TS: `mcpInfo?: { serverName: string; toolName: string }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_name: String,
    pub tool_name: String,
}

impl McpToolInfo {
    /// MCP-qualified tool name: `mcp__<server>__<tool>`.
    ///
    /// TS `toolExecution.ts:287-300` + `mcpStringUtils.ts`. This is the
    /// canonical name registered in the `ToolRegistry` so that MCP tools
    /// cannot accidentally shadow built-in tools — a hostile or buggy
    /// MCP server advertising a tool named `Read` or `Bash` gets
    /// namespaced as `mcp__foo__Read` instead of overwriting the real
    /// one.
    ///
    /// Server and tool names are passed through unchanged. Sanitization
    /// (replacing `-`/`.`/` ` with `_`) is the caller's responsibility
    /// if the upstream MCP server uses characters that would break the
    /// delimiter; most servers already use snake_case.
    pub fn qualified_name(&self) -> String {
        format!("mcp__{}__{}", self.server_name, self.tool_name)
    }
}

/// Progress update from a tool during execution.
///
/// TS: `ToolProgress<P>` — yielded immediately via onProgress callback.
/// In Rust, sent via `ctx.progress_tx` channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgress {
    pub tool_use_id: String,
    /// Optional parent tool use ID (for nested progress).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    /// Progress payload (tool-specific data).
    pub data: Value,
}

/// Sender for tool progress updates.
pub type ProgressSender = tokio::sync::mpsc::UnboundedSender<ToolProgress>;

/// Receiver for tool progress updates.
pub type ProgressReceiver = tokio::sync::mpsc::UnboundedReceiver<ToolProgress>;

/// The core Tool trait. All 41+ built-in tools implement this.
///
/// Maps to TS Tool interface. Execution follows:
/// validate_input -> check_permissions -> execute -> modify_context_after.
///
/// Progress reporting: Tools send progress via `ctx.progress_tx` channel
/// during execute(). The StreamingToolExecutor yields these immediately
/// to the TUI for real-time display.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    // -- Identity --

    /// Tool identity (ToolId::Builtin or Mcp or Custom).
    fn id(&self) -> ToolId;

    /// User-facing name (e.g., "Read", "Bash", "WebFetch").
    fn name(&self) -> &str;

    /// Alternative names for tool search.
    fn aliases(&self) -> &[&str] {
        &[]
    }

    /// Short search hint for ToolSearch deferred discovery (3-10 words).
    fn search_hint(&self) -> Option<&str> {
        None
    }

    // -- Schema --

    /// JSON schema for tool input parameters.
    fn input_schema(&self) -> ToolInputSchema;

    /// Optional JSON schema override (for tools with complex schemas).
    fn input_json_schema(&self) -> Option<Value> {
        None
    }

    /// Optional output schema for structured output validation.
    fn output_schema(&self) -> Option<Value> {
        None
    }

    /// Whether to enforce strict schema validation.
    fn strict(&self) -> bool {
        false
    }

    // -- Description --

    /// Dynamic description that may vary based on input and context.
    ///
    /// TS: `description(input, options)` — options provide session/tool context.
    fn description(&self, input: &Value, options: &DescriptionOptions) -> String;

    /// User-facing prompt description.
    ///
    /// TS: `prompt(options)` is async — tools may need permission context
    /// or other async data to generate their prompt. The default implementation
    /// delegates to the sync `description()` method.
    async fn prompt(&self, options: &PromptOptions) -> String {
        self.description(&Value::Null, &options.as_description_options())
    }

    /// User-facing display name (defaults to name()).
    fn user_facing_name(&self) -> &str {
        self.name()
    }

    // -- Capability Flags --

    /// Whether this tool is enabled (can be feature-gated).
    fn is_enabled(&self) -> bool {
        true
    }

    /// Whether this tool only reads (no side effects).
    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    /// Whether multiple instances can safely run concurrently.
    /// Critical for batch partitioning in StreamingToolExecutor.
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool performs destructive operations.
    fn is_destructive(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool should be deferred (lazy-loaded, discovered via ToolSearch).
    fn should_defer(&self) -> bool {
        false
    }

    /// Whether this tool should always be loaded (even when deferred).
    fn always_load(&self) -> bool {
        false
    }

    /// Whether this is an LSP tool.
    fn is_lsp(&self) -> bool {
        false
    }

    /// How this tool behaves when the user interrupts.
    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }

    /// Maximum result size in characters (default 100,000).
    fn max_result_size_chars(&self) -> i32 {
        100_000
    }

    /// MCP server/tool info (for MCP-wrapped tools).
    ///
    /// TS: `mcpInfo?: { serverName, toolName }` — identifies MCP origin.
    fn mcp_info(&self) -> Option<&McpToolInfo> {
        None
    }

    /// Whether this tool requires user interaction to complete.
    ///
    /// TS: `requiresUserInteraction?()` — defaults to true.
    /// When false, permission prompts are auto-denied for headless/background agents.
    /// Used by ExitPlanMode (returns false for teammates so they send approval
    /// via mailbox instead of requiring a local permission dialog).
    fn requires_user_interaction(&self) -> bool {
        true
    }

    /// Whether this tool exhibits "open-world" behavior — i.e. its effect
    /// depends on external state not under our control (environment,
    /// network, external services, arbitrary user input). Used as a
    /// metadata hint for UI rendering and telemetry; does NOT gate
    /// permissions or execution.
    ///
    /// TS: `Tool.ts:434` `isOpenWorld?(input) -> boolean`. TS uses this
    /// to tag MCP tools with an "[open-world]" label in the list view
    /// (`components/mcp/MCPToolListView.tsx:63`) and to set a `openWorld`
    /// field in `/print` CLI output (`cli/print.ts:1662`).
    ///
    /// Default is `false` — tools are closed-world unless they opt in.
    /// Dynamic MCP wrappers (`core/tools/src/tools/mcp_tools.rs`) can
    /// override this to forward the annotation from the MCP server.
    fn is_open_world(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool is sourced from an MCP (Model Context Protocol)
    /// server rather than being a native built-in.
    ///
    /// TS: `Tool.ts:436` `isMcp?: boolean`. TS uses this field to
    /// distinguish MCP-wrapped tools from built-ins for UI labeling,
    /// permission filtering, and the MCP list/detail views. The
    /// `isLsp?: boolean` sibling at `Tool.ts:437` serves the same role
    /// for LSP-backed tools — coco-rs has `is_lsp()` already; T3 adds
    /// the MCP counterpart.
    ///
    /// Default derives from `mcp_info()`: any tool that advertises
    /// `McpToolInfo` is an MCP tool. Concrete MCP wrapper
    /// implementations may still override this if they distinguish
    /// between pseudo-tools (e.g. MCP auth) and real MCP tools.
    fn is_mcp(&self) -> bool {
        self.mcp_info().is_some()
    }

    /// Returns information about whether this tool use is a search or read
    /// operation that should be collapsed into a condensed display in the UI.
    ///
    /// TS: `isSearchOrReadCommand?(input)` — returns `{ isSearch, isRead, isList? }`.
    fn is_search_or_read_command(&self, _input: &Value) -> Option<SearchReadInfo> {
        None
    }

    /// Returns a short string summary of this tool use for compact views.
    ///
    /// TS: `getToolUseSummary?(input)` — used by background agent progress display.
    fn get_tool_use_summary(&self, _input: &Value) -> Option<String> {
        None
    }

    /// Returns a human-readable present-tense activity description for spinner
    /// display (e.g., "Reading src/foo.ts", "Running bun test").
    ///
    /// TS: `getActivityDescription?(input)` — falls back to tool name if None.
    fn get_activity_description(&self, _input: &Value) -> Option<String> {
        None
    }

    /// Whether this tool is a transparent wrapper that delegates all rendering
    /// to its progress handler. The wrapper itself shows nothing in the UI.
    ///
    /// TS: `isTransparentWrapper?()` — used by REPL tool.
    fn is_transparent_wrapper(&self) -> bool {
        false
    }

    /// Returns flattened text of what the tool result shows, for transcript
    /// search indexing.
    ///
    /// TS: `extractSearchText?(output)` — optional, falls back to heuristic.
    fn extract_search_text(&self, _output: &Value) -> Option<String> {
        None
    }

    /// Returns true when the non-verbose rendering of this output is truncated
    /// (i.e., expanding would reveal more content).
    ///
    /// TS: `isResultTruncated?(output)` — gates click-to-expand in fullscreen.
    fn is_result_truncated(&self, _output: &Value) -> bool {
        false
    }

    // -- Validation --

    /// Validate input before execution. Called before check_permissions.
    ///
    /// TS: `validateInput(input, context)` — context needed for stateful
    /// validation like read-before-write enforcement.
    fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        ValidationResult::Valid
    }

    /// Check if two inputs are equivalent (for idempotency detection).
    fn inputs_equivalent(&self, _a: &Value, _b: &Value) -> bool {
        false
    }

    /// Backfill observable input fields for hooks/logging.
    ///
    /// TS: `backfillObservableInput(input)` — normalizes input before
    /// hooks see it (e.g., adds default field values, expands aliases).
    /// Called on a shallow clone; the original input is unchanged.
    fn backfill_observable_input(&self, _input: &mut Value) {}

    // -- Permissions --

    /// Check permissions for this tool invocation.
    async fn check_permissions(&self, _input: &Value, _ctx: &ToolUseContext) -> PermissionDecision {
        PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        }
    }

    /// Prepare a permission matcher string for hook matching.
    fn prepare_permission_matcher(&self, input: &Value) -> String {
        let _ = input;
        self.name().to_string()
    }

    /// Generate representation for auto-mode security classifier.
    fn to_auto_classifier_input(&self, input: &Value) -> String {
        let _ = input;
        self.name().to_string()
    }

    // -- Execution --

    /// Execute the tool.
    ///
    /// Progress reporting: send updates via `ctx.progress_tx` if available.
    /// ```ignore
    /// if let Some(tx) = &ctx.progress_tx {
    ///     let _ = tx.send(ToolProgress {
    ///         tool_use_id: ctx.tool_use_id.clone().unwrap_or_default(),
    ///         parent_tool_use_id: None,
    ///         data: serde_json::json!({"status": "running", "pct": 50}),
    ///     });
    /// }
    /// ```
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError>;

    /// Post-execution context modification.
    /// Called after execute() returns, before results are yielded.
    fn modify_context_after(&self, _result: &ToolResult<Value>, _ctx: &mut ToolUseContext) {}

    // -- File Path --

    /// Get the file path associated with this tool call (for file-based tools).
    fn get_path(&self, _input: &Value) -> Option<String> {
        None
    }

    // -- Result Mapping --

    /// Map tool result to API-compatible content blocks.
    fn map_tool_result_to_block(&self, result: &ToolResult<Value>) -> Vec<Message> {
        let _ = result;
        Vec::new()
    }
}

#[cfg(test)]
#[path = "traits.test.rs"]
mod tests;
