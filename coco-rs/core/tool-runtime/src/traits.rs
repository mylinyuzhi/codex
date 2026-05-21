use coco_messages::ToolResult;
use coco_messages::ToolResultContentPart;
use coco_types::ToolCheckResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::context::ToolUseContext;
use crate::error::ToolError;
use crate::validation::ValidationResult;

/// Session context for [`Tool::input_schema_for_session`]. Carries
/// the per-session knobs that drive TS-parity dynamic schema omits
/// (e.g. `AgentTool.tsx:110-125 lazySchema`'s
/// `isBackgroundTasksDisabled || isForkSubagentEnabled()` gate).
///
/// Constructed at the model-facing schema seam
/// (`engine_prompt::build_language_model_tools`) once per turn from
/// runtime config + features; tools read the fields they care about.
#[derive(Debug, Clone, Default)]
pub struct SchemaContext {
    /// True when `COCO_BACKGROUND_TASKS_DISABLE` env truthy. TS:
    /// `isBackgroundTasksDisabled`. AgentTool drops `run_in_background`
    /// from its schema when this is set.
    pub background_tasks_disabled: bool,
    /// True when fork-subagent mode is active for this session. TS:
    /// `isForkSubagentEnabled()`. AgentTool drops `run_in_background`
    /// when set — fork spawns always go through the bg path.
    pub fork_mode_active: bool,
    /// Snapshot of parent session features; tools that schema-gate
    /// on capability flags consult this. `None` when the seam can't
    /// resolve features (test / minimal SDK embedding).
    pub features: Option<std::sync::Arc<coco_types::Features>>,
}

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
/// TS: `prompt({ getToolPermissionContext, tools, agents, allowedAgentTypes, skills })`
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
    /// Names of model-invocable skills available this turn. Sorted
    /// for deterministic prompt text so tests and cache keys are
    /// stable. `SkillTool::prompt` consumes this to render the
    /// dynamic skill listing — `coco-rs` deliberately injects the
    /// list into the tool description rather than relying on system
    /// reminders, so every turn guarantees model visibility even if
    /// the reminder cadence skipped.
    pub skill_names: Vec<String>,
    /// Permission context for tailoring prompt to current mode.
    /// TS: `getToolPermissionContext()` — async in TS, pre-resolved here.
    pub permission_context: Option<coco_types::ToolPermissionContext>,
    /// Full agent catalog snapshot. `AgentTool::prompt` consumes this
    /// to render the per-agent listing (`- {type}: {whenToUse} (Tools:
    /// ...)`) so the model sees the available subagent types and their
    /// tool surfaces. `None` ⇒ static fallback description (the
    /// pre-Round-7 behaviour). TS parity: `AgentTool.tsx:218-225`
    /// passes `filterAgentsByMcpRequirements(agents, mcpServersWithTools)`
    /// to `getPrompt`.
    pub agent_catalog: Option<std::sync::Arc<coco_subagent::AgentCatalogSnapshot>>,
    /// Names of MCP servers ready (connected) this turn. The dynamic
    /// AgentTool prompt uses this to filter out agent definitions
    /// whose `required_mcp_servers` aren't all available — the model
    /// then never sees an agent it can't actually call. TS parity:
    /// `mcpServersWithTools` arg to `filterAgentsByMcpRequirements`.
    ///
    /// `None` ⇒ no MCP layer wired; the renderer's behaviour is to
    /// hide MCP-required agents (fail-closed). `Some(list)` filters
    /// against the named connected servers.
    pub ready_mcp_servers: Option<Vec<String>>,
    /// Coordinator-mode flag — when true, `AgentTool::prompt` renders
    /// the slim coordinator description (no usage notes, no parallel-
    /// spawn examples). TS parity: `isCoordinator` branch in
    /// `getPrompt`.
    pub coordinator_mode: bool,
    /// Fork-mode flag — when true, `AgentTool::prompt` adds the fork
    /// guidance section. TS parity: `isForkSubagentEnabled()` gating
    /// in `getPrompt`.
    pub fork_enabled: bool,
    /// Plan-mode interview-phase flag. When true, `EnterPlanModeTool::prompt`
    /// omits the `## What Happens in Plan Mode` section because the
    /// detailed iterative workflow already arrives via the plan-mode
    /// attachment. TS parity: `isPlanModeInterviewPhaseEnabled()` —
    /// in coco-rs the source is `settings.plan_mode.workflow ==
    /// Interview` only (no Growthbook / no `USER_TYPE=ant` / no env
    /// var; see `core/context/CLAUDE.md`).
    pub is_plan_interview_phase: bool,
    /// Host build embeds search tools (`bfs` / `ugrep`) inside the Bash
    /// tool. `AgentTool::prompt` swaps the "When NOT to use" section's
    /// FileRead/Glob/Grep hints for `find` / `grep` via Bash. TS parity:
    /// `hasEmbeddedSearchTools()` in `prompt.ts:222-231`.
    pub has_embedded_search_tools: bool,
    /// Parent session is itself an in-process teammate. Drops the
    /// run_in_background / name / team_name / mode bullets and adds the
    /// "only synchronous subagents" notice in the AgentTool prompt. TS
    /// parity: `isInProcessTeammate()` in `prompt.ts:277-279`.
    pub is_in_process_teammate: bool,
    /// Parent session is a (non in-process) teammate. Drops the name /
    /// team_name / mode bullets in the AgentTool prompt. TS parity:
    /// `isTeammate()` in `prompt.ts:280-282`.
    pub is_teammate: bool,
    /// Inject the agent listing into a system-reminder attachment
    /// instead of inline in the tool description. Stabilises the
    /// tools-block prompt cache against MCP / plugin / permission
    /// changes. TS parity: `shouldInjectAgentListInMessages()` in
    /// `prompt.ts:59-64` (env `COCO_AGENT_LIST_IN_MESSAGES`).
    pub agent_list_via_attachment: bool,
    /// Pro subscriptions skip the inline "Launch multiple agents
    /// concurrently" usage-notes bullet because the same guidance is
    /// shown by the agent_listing_delta attachment for them. TS parity:
    /// `getSubscriptionType() !== 'pro'` in `prompt.ts:246`.
    pub is_pro_subscription: bool,
    /// Host disabled background tasks via
    /// `COCO_BACKGROUND_TASKS_DISABLE`. Suppresses the run_in_background
    /// paragraphs in AgentTool's prompt. TS parity:
    /// `process.env.CLAUDE_CODE_DISABLE_BACKGROUND_TASKS` in
    /// `prompt.ts:259`.
    pub background_tasks_disabled: bool,
    /// Internal-build flag enabling the `isolation: "remote"` bullet.
    /// 3p builds keep this off because coco-rs ships only the local
    /// `worktree` isolation runtime. TS parity: `process.env.USER_TYPE
    /// === 'ant'` in `prompt.ts:273`.
    pub ant_build: bool,
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
        use coco_types::MCP_TOOL_PREFIX;
        use coco_types::MCP_TOOL_SEPARATOR;
        format!(
            "{MCP_TOOL_PREFIX}{server}{MCP_TOOL_SEPARATOR}{tool}",
            server = self.server_name,
            tool = self.tool_name,
        )
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

/// The core Tool trait. All built-in tools implement this.
///
/// Maps to TS Tool interface. Execution follows:
/// validate_input -> check_permissions -> execute.
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

    /// Session-aware variant. Default impl just returns the static
    /// [`Self::input_schema`]; tools whose schema depends on
    /// per-session flags (env var killswitches, fork-mode gates,
    /// feature toggles) override this to emit a variant.
    ///
    /// TS parity: `AgentTool.tsx:110-125 lazySchema` rebuilds the
    /// zod schema per-call and conditionally `.omit({...})`s
    /// fields when `isBackgroundTasksDisabled` or
    /// `isForkSubagentEnabled()` flips. Without a session-aware
    /// schema the model is told a field exists (e.g.
    /// `run_in_background`) when the runtime would silently
    /// override it — a schema-honesty gap.
    ///
    /// `engine_prompt::build_language_model_tools` calls this
    /// instead of [`Self::input_schema`] so the model-facing
    /// schema matches the actual runtime contract for the session.
    fn input_schema_for_session(&self, _ctx: &SchemaContext) -> ToolInputSchema {
        self.input_schema()
    }

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

    /// Whether this tool is enabled in the given context.
    ///
    /// Default returns `true` — most tools are always available. Override
    /// to gate the tool on `ctx.features.enabled(Feature::X)` (token-economy
    /// or experimental gates), an OS check, or a runtime resource probe.
    /// See `docs/coco-rs/feature-gates-and-tool-filtering.md` for the
    /// design and the multi-layer filter pipeline this hook is the
    /// first layer of.
    fn is_enabled(&self, _ctx: &crate::context::ToolUseContext) -> bool {
        true
    }

    /// Whether this tool only reads (no side effects).
    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool is **statically** read-only — known to be safe
    /// without inspecting input. Used by Layer 3 (`PermissionMode::Plan`)
    /// to filter the schema at definitions-time, before any input exists.
    /// See `docs/coco-rs/feature-gates-and-tool-filtering.md` §7.
    ///
    /// **Contract**: the answer must not depend on input. Tools whose
    /// read/write nature genuinely varies with input (e.g. `Bash`)
    /// **must** leave the default — Plan mode then hides them.
    ///
    /// Default delegates to `is_read_only(&Value::Null)` so the common
    /// case — tools whose `is_read_only` impl ignores input and returns
    /// a constant — gets the correct answer for free without an extra
    /// override. Tools whose `is_read_only` *consults* the input must
    /// override `is_always_read_only` to return `false` explicitly.
    fn is_always_read_only(&self) -> bool {
        self.is_read_only(&Value::Null)
    }

    /// Whether multiple instances can safely run concurrently.
    /// Critical for batch partitioning in StreamingToolExecutor.
    ///
    /// **Invariant**: tools returning `true` MUST NOT mutate
    /// `ctx.app_state` during `execute`. Concurrent tools share a
    /// single `Arc<RwLock<ToolAppState>>`; live writes would race
    /// with sibling reads. TS parity: `orchestration.ts:30-62` runs
    /// concurrent tools against a shared `currentContext` snapshot
    /// and *queues* `setAppState` calls to apply after the batch.
    /// Rust relies on convention (concurrent tools are read-only —
    /// Read/Glob/Grep/LSP/etc.) instead of implementing the queue;
    /// this comment is the contract. Serial unsafe tools
    /// (`is_concurrency_safe == false`) are the only code path that
    /// writes `ctx.app_state`.
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

    /// Per-tool tool-result size cap (in characters). When a tool's
    /// result exceeds this size, the runtime persists it to disk
    /// (via `coco_tool_runtime::tool_result_storage::persist_to_disk`)
    /// and substitutes a `<persisted-output>` reference message into
    /// the conversation.
    ///
    /// `i64::MAX` opts the tool out of persistence — its results stay
    /// inline regardless of length. Tools whose
    /// output is canonical (e.g. `Read` on a tracked file the model
    /// will read again) opt out so persistence isn't circular.
    ///
    /// TS: `Tool.maxResultSizeChars` (default `100_000`, clamped by
    /// `DEFAULT_MAX_RESULT_SIZE_CHARS = 50_000`). Override per-tool to
    /// declare opt-out ([`ResultSizeBound::Unbounded`]) or a tighter cap
    /// ([`ResultSizeBound::Chars`]).
    fn max_result_size_bound(&self) -> crate::tool_result_storage::ResultSizeBound {
        crate::tool_result_storage::DEFAULT_TOOL_MAX_RESULT_SIZE_BOUND
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

    /// Tool's own opinion at the central evaluator's step-1c slot.
    ///
    /// TS parity: `tool.checkPermissions(parsedInput, context)` in
    /// `permissions.ts`. Tools that need content-specific safety
    /// checks (Read/Grep/Glob path safety, Bash subcommand parsing,
    /// Write path validation) override this to return `Deny`/`Ask`
    /// for unsafe inputs and `Passthrough` otherwise. The default
    /// `Passthrough` defers entirely to the rule pipeline; this
    /// matches TS where tools without a `checkPermissions` impl
    /// behave the same as `() => ({ behavior: 'passthrough' })`.
    ///
    /// The result is consumed by
    /// `coco_permissions::PermissionEvaluator::evaluate_with_tool_check`
    /// inside `app/query::tool_call_preparer::resolve_permission_decision`.
    /// Returning `Allow { updated_input }` here propagates the
    /// normalized input onto the resulting `PermissionDecision::Allow`.
    async fn check_permissions(&self, _input: &Value, _ctx: &ToolUseContext) -> ToolCheckResult {
        ToolCheckResult::Passthrough
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

    // -- File Path --

    /// Get the file path associated with this tool call (for file-based tools).
    fn get_path(&self, _input: &Value) -> Option<String> {
        None
    }

    // -- Result Mapping --

    /// Render the tool's structured output as a sequence of content
    /// parts (text + images + documents) that the model sees in the
    /// `tool_result` block.
    ///
    /// TS parity: `mapToolResultToToolResultBlockParam(data, toolUseId)`
    /// in every TS Tool. The Rust signature drops `tool_use_id`
    /// because the executor wraps the parts at message-creation time,
    /// not the tool.
    ///
    /// # Default behaviour
    ///
    /// The default impl emits a single [`ToolResultContentPart::Text`]
    /// with `serde_json::to_string(&data)` — byte-identical to the
    /// pre-`render_for_model` codepath that did
    /// `serde_json::to_string(&output_data)` in
    /// `app/query/src/tool_outcome_builder.rs`. Tools opt into custom
    /// rendering (token efficiency, multimodal images, etc.) by
    /// overriding.
    ///
    /// # Path 1 only
    ///
    /// This method is **only** for tool results — i.e. content blocks
    /// that pair with a real `tool_use_id` from the assistant's prior
    /// `tool_use`. Synthesizing a `tool_result` without a paired
    /// `tool_use_id` is rejected by every major provider (Anthropic
    /// 400, OpenAI "Invalid parameter…", Gemini schema mismatch).
    /// Slash command output, system reminders, and attachments use
    /// their own user-message-text paths and do NOT go through here.
    ///
    /// # Provider degradation
    ///
    /// Multi-block parts (image / document) flow through to providers
    /// that support them (Anthropic, Gemini 3+). Text-only providers
    /// (OpenAI Chat, OpenAI-Compatible: DeepSeek/xAI/Groq) replace
    /// non-Text parts with a visible marker at the provider boundary
    /// — see `vercel-ai-openai/src/messages/...` for the conversion.
    ///
    /// # Purity
    ///
    /// Implementations must be pure (no IO, no async, no global
    /// state). If a tool needs async work to format its output, do
    /// the work in [`Tool::execute`] and stash the rendered form in
    /// `data`.
    ///
    /// # Common pattern
    ///
    /// Tools whose `data` is already the human-readable string (e.g.
    /// Glob, Grep, ListMcpResources) call [`render_text_or_json`]
    /// instead of duplicating the bare-string-or-JSON unwrap
    /// boilerplate.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: serde_json::to_string(data).unwrap_or_default(),
            provider_options: None,
        }]
    }
}

/// Helper for the common `render_for_model` pattern: emit a single
/// [`ToolResultContentPart::Text`] containing either the bare string
/// payload (when `data` is `Value::String`) or the JSON-stringified
/// `data` for any other shape.
///
/// This is what TS tools whose `mapToolResultToToolResultBlockParam`
/// returns plain text do — the model sees the underlying message
/// without a `"…"` JSON-quote wrapper. Tools that already build their
/// confirmation string in `execute()` (Glob, Grep, MCP*, AskUserQuestion,
/// SendMessage, …) use this so they don't each carry the same
/// 6-line `data.as_str().map(...).unwrap_or_else(...)` boilerplate.
///
/// Tools needing custom branches (Bash, Read, Edit, plan-mode, agent,
/// scheduling, brief, …) override `Tool::render_for_model` directly.
pub fn render_text_or_json(data: &Value) -> Vec<ToolResultContentPart> {
    let text = data
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default());
    vec![ToolResultContentPart::Text {
        text,
        provider_options: None,
    }]
}

#[cfg(test)]
#[path = "traits.test.rs"]
mod tests;
