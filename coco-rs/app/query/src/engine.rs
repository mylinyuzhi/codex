//! The agent loop — heart of the system.
//!
//! TS: QueryEngine.ts + query.ts
//!
//! State transitions tracked via ContinueReason to enable tests to verify
//! recovery paths without inspecting message contents.

use crate::budget::BudgetDecision;
use crate::budget::BudgetTracker;
use crate::command_queue::CommandQueue;
use crate::command_queue::Inbox;
use crate::command_queue::QueuePriority;
use crate::emit::emit_protocol;
use crate::emit::emit_protocol_owned;
use crate::emit::emit_stream;
use crate::session_state::SessionStateTracker;
use coco_context::FileHistoryState;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration;
use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_messages::CostTracker;
use coco_messages::MessageHistory;
use coco_tool::PendingToolCall;
use coco_tool::StreamingToolExecutor;
use coco_tool::ToolRegistry;
use coco_tool::ToolUseContext;
use coco_types::AssistantContent;
use coco_types::HookEventType;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::PermissionDecision;
use coco_types::PermissionMode;
use coco_types::TokenUsage;
use coco_types::ToolId;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::language_model::v4::LanguageModelV4FunctionTool;

/// Why the loop is continuing instead of exiting.
///
/// TS: Continue type union in query.ts — enables tests to verify recovery
/// paths fired without inspecting message contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinueReason {
    /// Normal tool-call loop: model returned tool calls, process and continue.
    NextTurn,
    /// Reactive compaction after prompt-too-long error.
    ReactiveCompactRetry,
    /// Max output tokens escalation (try 64k).
    MaxOutputTokensEscalate,
    /// Max output tokens recovery attempt.
    MaxOutputTokensRecovery { attempt: i32 },
    /// Stop hook requested blocking continuation.
    StopHookBlocking,
    /// Token budget allows one more continuation.
    TokenBudgetContinuation,
    /// Context collapse drain retry.
    CollapseDrainRetry { committed: i32 },
}

/// Configuration for the query engine.
#[derive(Debug, Clone)]
pub struct QueryEngineConfig {
    /// Maximum turns before stopping.
    pub max_turns: i32,
    /// Maximum output tokens per request.
    pub max_tokens: Option<i64>,
    /// System prompt to prepend.
    pub system_prompt: Option<String>,
    /// Append to system prompt (after CLAUDE.md).
    pub append_system_prompt: Option<String>,
    /// Model name for tool context.
    pub model_name: String,
    /// Fallback model for error recovery.
    pub fallback_model: Option<String>,
    /// Permission mode for tool execution.
    pub permission_mode: PermissionMode,
    /// Context window size in tokens (for compaction trigger).
    pub context_window: i64,
    /// Max output tokens for the model (used in effective window calculation).
    pub max_output_tokens: i64,
    /// Maximum budget in USD (None = unlimited).
    pub max_budget_usd: Option<f64>,
    /// Enable streaming tool execution (tools execute during API streaming).
    pub streaming_tool_execution: bool,
    /// Whether this is a non-interactive (SDK/script) session.
    pub is_non_interactive: bool,
    /// Session identifier for hook orchestration context.
    pub session_id: String,
    /// Project root directory for hook orchestration context.
    pub project_dir: Option<std::path::PathBuf>,
    /// Disable all hooks (from settings).
    pub disable_all_hooks: bool,
    /// Only allow managed/policy hooks (from settings).
    pub allow_managed_hooks_only: bool,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_turns: 30,
            max_tokens: None,
            system_prompt: None,
            append_system_prompt: None,
            model_name: String::new(),
            fallback_model: None,
            permission_mode: PermissionMode::Default,
            context_window: 200_000,
            max_output_tokens: 16_384,
            max_budget_usd: None,
            streaming_tool_execution: true,
            is_non_interactive: false,
            session_id: String::new(),
            project_dir: None,
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
        }
    }
}

/// One-shot bootstrap data for `SessionStarted` emission.
///
/// Collected by the CLI layer at session start and handed to the engine so it
/// can emit a single `CoreEvent::Protocol(ServerNotification::SessionStarted)`
/// with full context before the first turn.
///
/// TS equivalent: `buildSystemInitMessage()` in
/// `src/utils/messages/systemInit.ts`. Fields mirror
/// `SDKSystemMessageSchema` init subtype (coreSchemas.ts:1457-1494).
#[derive(Debug, Clone, Default)]
pub struct SessionBootstrap {
    pub protocol_version: String,
    pub cwd: String,
    pub version: String,
    /// Tool names the LLM will see. If empty, the engine falls back to
    /// `ToolRegistry::loaded_tools()` names.
    pub tools: Vec<String>,
    pub slash_commands: Vec<String>,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<coco_types::McpServerInit>,
    pub plugins: Vec<coco_types::PluginInit>,
    pub api_key_source: Option<String>,
    pub betas: Vec<String>,
    pub output_style: Option<String>,
    pub fast_mode_state: Option<coco_types::FastModeState>,
}

/// Result of running the query engine.
#[derive(Debug)]
pub struct QueryResult {
    /// Final assistant text response.
    pub response_text: String,
    /// Total turns executed.
    pub turns: i32,
    /// Accumulated token usage.
    pub total_usage: TokenUsage,
    /// Per-model cost tracking.
    pub cost_tracker: CostTracker,
    /// Whether the engine was cancelled.
    pub cancelled: bool,
    /// Whether the budget was exhausted.
    pub budget_exhausted: bool,
    /// Why the engine stopped (last continue reason or None for clean exit).
    pub last_continue_reason: Option<ContinueReason>,
    /// Total duration in milliseconds.
    pub duration_ms: i64,
    /// Total API time in milliseconds.
    pub duration_api_ms: i64,
    /// Stop reason from the model.
    pub stop_reason: Option<String>,
    /// Permission denials accumulated during the session. Populated on each
    /// `PermissionDecision::Deny` branch in the tool execution loop and
    /// flushed into `SessionResultParams` at session end.
    /// Matches TS `SDKPermissionDenial` array (coreSchemas.ts:1399-1405).
    pub permission_denials: Vec<coco_types::PermissionDenialInfo>,
    /// Final message history at the end of the turn, including the
    /// user prompt, any tool calls + results, and the final assistant
    /// reply. Used by multi-turn SDK sessions to thread context
    /// forward into the next `turn/start`.
    pub final_messages: Vec<Message>,
}

/// The query engine — orchestrates multi-turn agent conversations.
pub struct QueryEngine {
    config: QueryEngineConfig,
    client: Arc<ApiClient>,
    tools: Arc<ToolRegistry>,
    cancel: CancellationToken,
    hooks: Option<Arc<HookRegistry>>,
    /// Mid-turn command queue for steering.
    command_queue: CommandQueue,
    /// Inbox for teammate messages.
    inbox: Inbox,
    /// Session-level file read state for @mention dedup and changed-file detection.
    file_read_state: Option<Arc<RwLock<coco_context::FileReadState>>>,
    /// File history for checkpoint/rewind.
    /// TS: fileHistoryState in AppState + callbacks in toolUseContext.
    file_history: Option<Arc<RwLock<FileHistoryState>>>,
    /// Config home directory for file history backup storage.
    config_home: Option<std::path::PathBuf>,
    /// One-shot SessionStarted payload; emitted at the first turn entry.
    session_bootstrap: Option<SessionBootstrap>,
    /// Optional permission bridge for routing `PermissionDecision::Ask`
    /// outcomes to an external authority (swarm leader or SDK client).
    /// `None` uses the engine's fallback auto-allow behavior.
    permission_bridge: Option<coco_tool::ToolPermissionBridgeRef>,
}

impl QueryEngine {
    pub fn new(
        config: QueryEngineConfig,
        client: Arc<ApiClient>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        hooks: Option<Arc<HookRegistry>>,
    ) -> Self {
        Self {
            config,
            client,
            tools,
            cancel,
            hooks,
            command_queue: CommandQueue::new(),
            inbox: Inbox::new(),
            file_read_state: None,
            file_history: None,
            config_home: None,
            session_bootstrap: None,
            permission_bridge: None,
        }
    }

    /// Attach session bootstrap data to be emitted as `SessionStarted`
    /// before the first turn. Without this, the engine still runs normally
    /// but does not emit `SessionStarted` (backwards compatible for tests).
    pub fn with_session_bootstrap(mut self, bootstrap: SessionBootstrap) -> Self {
        self.session_bootstrap = Some(bootstrap);
        self
    }

    /// Attach a permission bridge so `PermissionDecision::Ask` outcomes
    /// are forwarded to an external authority (e.g. the SDK client via
    /// `SdkPermissionBridge`) instead of auto-allowing.
    pub fn with_permission_bridge(mut self, bridge: coco_tool::ToolPermissionBridgeRef) -> Self {
        self.permission_bridge = Some(bridge);
        self
    }

    /// Set file read state for @mention dedup and changed-file detection.
    pub fn with_file_read_state(
        mut self,
        file_read_state: Arc<RwLock<coco_context::FileReadState>>,
    ) -> Self {
        self.file_read_state = Some(file_read_state);
        self
    }

    /// Set file history state for checkpoint/rewind support.
    pub fn with_file_history(
        mut self,
        file_history: Arc<RwLock<FileHistoryState>>,
        config_home: std::path::PathBuf,
    ) -> Self {
        self.file_history = Some(file_history);
        self.config_home = Some(config_home);
        self
    }

    /// Access the command queue for mid-turn steering.
    pub fn command_queue(&self) -> &CommandQueue {
        &self.command_queue
    }

    /// Access the inbox for teammate messages.
    pub fn inbox(&self) -> &Inbox {
        &self.inbox
    }

    /// Run the agent loop with event streaming from a text prompt.
    pub async fn run_with_events(
        &self,
        user_prompt: &str,
        event_tx: tokio::sync::mpsc::Sender<crate::CoreEvent>,
    ) -> anyhow::Result<QueryResult> {
        let user_msg = coco_messages::create_user_message(user_prompt);
        self.run_internal_with_messages(vec![user_msg], Some(event_tx))
            .await
    }

    /// Run the agent loop with pre-built messages (user + attachment messages).
    pub async fn run_with_messages(
        &self,
        messages: Vec<Message>,
        event_tx: tokio::sync::mpsc::Sender<crate::CoreEvent>,
    ) -> anyhow::Result<QueryResult> {
        if messages.is_empty() {
            anyhow::bail!("No messages to process");
        }
        self.run_internal_with_messages(messages, Some(event_tx))
            .await
    }

    /// Run the agent loop with an initial user prompt (no event streaming).
    pub async fn run(&self, user_prompt: &str) -> anyhow::Result<QueryResult> {
        let user_msg = coco_messages::create_user_message(user_prompt);
        self.run_internal_with_messages(vec![user_msg], None).await
    }

    /// Core internal implementation: user + attachment messages.
    ///
    /// First message is the user message (used for file history snapshot UUID).
    /// Subsequent messages are attachment messages (is_meta=true, system-reminder wrapped).
    ///
    /// Session lifecycle sequence (matches TS print.ts + QueryEngine.ts):
    /// 1. SessionStarted  (if bootstrap attached)   — TS: buildSystemInitMessage
    /// 2. SessionStateChanged(Running)              — TS: notifySessionStateChanged('running')
    /// 3. run_session_loop: turn-by-turn work       — TS: query() generator loop
    /// 4. SessionStateChanged(Idle)                 — TS: notifySessionStateChanged('idle')
    /// 5. SessionResult (success or error subtype)  — TS: SDKResultMessage
    ///
    /// Steps 1/2/4/5 fire regardless of success or error so SDK consumers
    /// always see a complete session envelope.
    async fn run_internal_with_messages(
        &self,
        turn_messages: Vec<Message>,
        event_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    ) -> anyhow::Result<QueryResult> {
        // Single choke point for all SessionStateChanged emissions. Dedupes
        // consecutive identical states so the wire stream sees exactly one
        // emission per real edge. See plan file WS-4.
        let state_tracker = SessionStateTracker::new();

        // SessionStarted must happen before anything that can error.
        self.emit_session_started(&event_tx).await;

        // Running — agent is actively processing.
        state_tracker
            .transition_to(coco_types::SessionState::Running, &event_tx)
            .await;

        // Set up the Hook → CoreEvent forwarder as a structured child task.
        //
        // The forwarder is a `JoinHandle` owned by this function, cancelled
        // via a child `CancellationToken` off `self.cancel`, and drained at
        // the single exit point below. See plan file WS-5.
        //
        // TS: print.ts emits SDKHookStartedMessage/etc. directly from the
        // hook execution path; in Rust we use this child task so
        // orchestration stays independent of the coco-query event type.
        let hook_cancel = self.cancel.child_token();
        let (hook_tx_opt, hook_forwarder_handle) = if event_tx.is_some() {
            let (hook_event_tx, hook_event_rx) =
                tokio::sync::mpsc::channel::<coco_hooks::HookExecutionEvent>(64);
            let core_tx = event_tx.clone();
            let handle = tokio::spawn(Self::forward_hook_events(
                hook_event_rx,
                core_tx,
                hook_cancel.clone(),
            ));
            (Some(hook_event_tx), Some(handle))
        } else {
            (None, None)
        };

        let result = self
            .run_session_loop(
                turn_messages,
                event_tx.clone(),
                &state_tracker,
                hook_tx_opt.clone(),
            )
            .await;

        // Drain the hook forwarder before emitting Idle/SessionResult so any
        // in-flight hook events land on the wire before the session
        // terminator. Order matters: drop the sender FIRST so the
        // forwarder's `rx.recv()` sees channel-closed; then await the
        // handle with a bounded timeout so a runaway hook can't wedge
        // shutdown.
        drop(hook_tx_opt);
        if let Some(handle) = hook_forwarder_handle {
            const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
            match tokio::time::timeout(DRAIN_TIMEOUT, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(join_err)) => {
                    warn!(error = %join_err, "hook forwarder task panicked");
                }
                Err(_) => {
                    warn!("hook forwarder drain timed out; cancelling");
                    hook_cancel.cancel();
                }
            }
        }

        // Idle — turn-over signal; emit regardless of outcome.
        state_tracker
            .transition_to(coco_types::SessionState::Idle, &event_tx)
            .await;

        // SessionResult — always emitted. On Err, we synthesize a minimal
        // QueryResult-like view so SDK consumers see a terminal `result`
        // event matching TS SDKResultErrorMessage.
        let params = match &result {
            Ok(qr) => self.build_session_result_params(qr, /*error_messages*/ Vec::new()),
            Err(e) => self.build_session_error_params(e.to_string()),
        };
        let _delivered = emit_protocol(
            &event_tx,
            crate::ServerNotification::SessionResult(Box::new(params)),
        )
        .await;

        result
    }

    /// Emit the `SessionStarted` protocol event from attached bootstrap data.
    /// No-op if the engine was not built with `with_session_bootstrap()`.
    async fn emit_session_started(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    ) {
        let Some(bootstrap) = &self.session_bootstrap else {
            return;
        };
        // Wire format is whatever `PermissionMode`'s serde serialization
        // produces — now camelCase matching TS `PermissionModeSchema`.
        let permission_mode = serde_json::to_value(self.config.permission_mode)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "default".into());
        let tools = if bootstrap.tools.is_empty() {
            self.tools
                .loaded_tools()
                .iter()
                .map(|t| t.name().to_string())
                .collect()
        } else {
            bootstrap.tools.clone()
        };
        let _delivered = emit_protocol(
            event_tx,
            crate::ServerNotification::SessionStarted(coco_types::SessionStartedParams {
                session_id: self.config.session_id.clone(),
                protocol_version: bootstrap.protocol_version.clone(),
                cwd: bootstrap.cwd.clone(),
                model: self.config.model_name.clone(),
                permission_mode,
                tools,
                slash_commands: bootstrap.slash_commands.clone(),
                agents: bootstrap.agents.clone(),
                skills: bootstrap.skills.clone(),
                mcp_servers: bootstrap.mcp_servers.clone(),
                plugins: bootstrap.plugins.clone(),
                api_key_source: bootstrap.api_key_source.clone(),
                betas: bootstrap.betas.clone(),
                version: bootstrap.version.clone(),
                output_style: bootstrap.output_style.clone(),
                fast_mode_state: bootstrap.fast_mode_state,
            }),
        )
        .await;
    }

    /// Synthesize a `SessionResultParams` for the error path (when
    /// `run_session_loop` returned `Err`). Matches TS `SDKResultErrorSchema`.
    fn build_session_error_params(&self, error_msg: String) -> coco_types::SessionResultParams {
        coco_types::SessionResultParams {
            session_id: self.config.session_id.clone(),
            total_turns: 0,
            duration_ms: 0,
            duration_api_ms: 0,
            is_error: true,
            stop_reason: "error_during_execution".into(),
            total_cost_usd: 0.0,
            usage: TokenUsage::default(),
            model_usage: Default::default(),
            permission_denials: Vec::new(),
            result: None,
            errors: vec![error_msg],
            structured_output: None,
            fast_mode_state: None,
            num_api_calls: None,
        }
    }

    /// Build a `SessionResultParams` from a completed `QueryResult`.
    /// Matches TS `SDKResultMessage` shape (coreSchemas.ts:1407-1451).
    ///
    /// `error_messages` is propagated into the `errors` field (for TS
    /// `SDKResultErrorSchema` parity); success results pass an empty Vec.
    fn build_session_result_params(
        &self,
        qr: &QueryResult,
        error_messages: Vec<String>,
    ) -> coco_types::SessionResultParams {
        // Per-model usage aggregated from CostTracker.
        let model_usage = qr
            .cost_tracker
            .per_model
            .iter()
            .map(|(model, usage)| {
                (
                    model.clone(),
                    coco_types::SessionModelUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cache_read_input_tokens: usage.cache_read_input_tokens,
                        cache_creation_input_tokens: usage.cache_creation_input_tokens,
                        web_search_requests: usage.web_search_requests,
                        cost_usd: usage.cost_usd,
                        context_window: self.config.context_window,
                        max_output_tokens: self.config.max_output_tokens,
                    },
                )
            })
            .collect();

        let stop_reason = qr
            .stop_reason
            .clone()
            .unwrap_or_else(|| "end_turn".to_string());
        let is_error = qr.cancelled || qr.budget_exhausted || !error_messages.is_empty();

        coco_types::SessionResultParams {
            session_id: self.config.session_id.clone(),
            total_turns: qr.turns,
            duration_ms: qr.duration_ms,
            duration_api_ms: qr.duration_api_ms,
            is_error,
            stop_reason,
            total_cost_usd: qr.cost_tracker.total_cost_usd(),
            usage: qr.total_usage,
            model_usage,
            // Accumulated across PermissionDecision::Deny branches.
            permission_denials: qr.permission_denials.clone(),
            result: if is_error {
                None
            } else {
                Some(qr.response_text.clone())
            },
            errors: error_messages,
            structured_output: None,
            fast_mode_state: None,
            num_api_calls: Some(qr.cost_tracker.total_api_calls as i32),
        }
    }

    async fn run_session_loop(
        &self,
        turn_messages: Vec<Message>,
        event_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        state_tracker: &SessionStateTracker,
        hook_tx_opt: Option<tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
    ) -> anyhow::Result<QueryResult> {
        let start_time = std::time::Instant::now();
        let mut api_time_ms: i64 = 0;
        let mut history = MessageHistory::new();
        let mut total_usage = TokenUsage::default();
        let mut cost_tracker = CostTracker::new();
        let mut turn = 0;
        let mut last_continue_reason: Option<ContinueReason> = None;
        let mut budget = BudgetTracker::new(
            self.config.max_tokens,
            self.config.max_turns,
            /*max_continuations*/ 3,
        );
        // The "current turn" user message id is the LAST user message in
        // `turn_messages`. In single-turn mode the list is
        // `[user_msg, attachment, ...]` and the first (and only) user
        // message is also the last. In multi-turn SDK mode the list is
        // `[prior_history..., new_user_msg]`, so the LAST user message
        // is the current turn's prompt — which is what file history
        // snapshots should key on.
        let user_msg_uuid = turn_messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User(u) => Some(u.uuid.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        for msg in turn_messages {
            history.push(msg);
        }

        // NOTE: `SessionStarted` + `SessionStateChanged(Running)` + the
        // hook → CoreEvent forwarder are set up by the outer
        // `run_internal_with_messages` BEFORE calling this function, so
        // SDK consumers see them even if the session loop errors out
        // before its first turn. See TS `runHeadless()` which initializes
        // the init message at the very top of the entry function.

        // Create file history snapshot for this user message.
        // TS: fileHistoryMakeSnapshot() in handlePromptSubmit.ts + QueryEngine.ts
        if let (Some(fh), Some(ch)) = (&self.file_history, &self.config_home) {
            let mut fh = fh.write().await;
            if let Err(e) = fh
                .make_snapshot(&user_msg_uuid, ch, &self.config.session_id)
                .await
            {
                warn!("file history make_snapshot failed: {e}");
            }
        }

        // Permission denials accumulated across all tool calls in this session.
        // Populated on each `PermissionDecision::Deny` branch and flushed
        // into `SessionResultParams.permission_denials` via the `make_result`
        // closure. Matches TS `QueryEngine.permissionDenials` wrapper
        // behavior (QueryEngine.ts:244-271).
        let mut permission_denials: Vec<coco_types::PermissionDenialInfo> = Vec::new();

        let make_result = |response_text: String,
                           turns: i32,
                           total_usage: TokenUsage,
                           cost_tracker: CostTracker,
                           cancelled: bool,
                           budget_exhausted: bool,
                           last_continue_reason: Option<ContinueReason>,
                           start_time: std::time::Instant,
                           api_time_ms: i64,
                           stop_reason: Option<String>,
                           permission_denials: Vec<coco_types::PermissionDenialInfo>,
                           final_messages: Vec<Message>| {
            QueryResult {
                response_text,
                turns,
                total_usage,
                cost_tracker,
                cancelled,
                budget_exhausted,
                last_continue_reason,
                duration_ms: start_time.elapsed().as_millis() as i64,
                duration_api_ms: api_time_ms,
                stop_reason,
                permission_denials,
                final_messages,
            }
        };

        loop {
            if self.cancel.is_cancelled() {
                return Ok(make_result(
                    String::new(),
                    turn,
                    total_usage,
                    cost_tracker,
                    /*cancelled*/ true,
                    /*budget_exhausted*/ false,
                    last_continue_reason,
                    start_time,
                    api_time_ms,
                    Some("cancelled".into()),
                    permission_denials,
                    history.messages.clone(),
                ));
            }

            // Budget check before each turn
            match budget.check(turn) {
                BudgetDecision::Stop { reason } => {
                    warn!(%reason, "budget stop");
                    let last_text = extract_last_assistant_text(&history);
                    return Ok(make_result(
                        last_text,
                        turn,
                        total_usage,
                        cost_tracker,
                        /*cancelled*/ false,
                        /*budget_exhausted*/ true,
                        last_continue_reason,
                        start_time,
                        api_time_ms,
                        Some("budget_exhausted".into()),
                        permission_denials,
                        history.messages.clone(),
                    ));
                }
                BudgetDecision::Nudge { message } => {
                    info!(%message, "budget nudge");
                    // No direct ServerNotification for budget nudge; emit as non-retryable Error
                    // so SDK consumers can surface the warning.
                    let _delivered = emit_protocol(
                        &event_tx,
                        crate::ServerNotification::Error(coco_types::ErrorParams {
                            message,
                            category: Some("budget".into()),
                            retryable: false,
                        }),
                    )
                    .await;
                }
                BudgetDecision::Continue => {}
            }

            turn += 1;
            info!(turn, "starting turn");
            let turn_id = format!("turn-{turn}");
            let _delivered = emit_protocol(
                &event_tx,
                crate::ServerNotification::TurnStarted(coco_types::TurnStartedParams {
                    turn_id: Some(turn_id.clone()),
                    turn_number: turn,
                }),
            )
            .await;

            // Build prompt from history
            let prompt = self.build_prompt(&history);
            let tool_defs = self.build_tool_definitions();

            // StreamRequestStart has no direct protocol equivalent; it was
            // previously only used for test classification. The model_name is
            // already carried in SessionStarted at session init.

            // Call LLM
            let params = QueryParams {
                prompt,
                max_tokens: self.config.max_tokens,
                thinking_level: None,
                fast_mode: false,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
            };

            let api_start = std::time::Instant::now();
            let llm_result = match self.client.query(&params).await {
                Ok(result) => result,
                Err(e) => {
                    let err_msg = e.to_string();
                    // Reactive compaction: if prompt too long, compact and retry.
                    if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                        warn!("prompt too long, attempting reactive compaction");
                        let drop_target = coco_compact::reactive::calculate_drop_target(
                            coco_compact::estimate_tokens(&history.messages),
                            &coco_compact::ReactiveCompactConfig {
                                context_window: self.config.context_window,
                                max_output_tokens: self.config.max_output_tokens,
                                ..Default::default()
                            },
                        );
                        coco_compact::reactive::api_microcompact(
                            &mut history.messages,
                            drop_target,
                        );
                        let _delivered =
                            emit_protocol(&event_tx, crate::ServerNotification::CompactionStarted)
                                .await;
                        last_continue_reason = Some(ContinueReason::ReactiveCompactRetry);
                        budget.reset_continuations();
                        continue;
                    }
                    return Err(anyhow::anyhow!("LLM query failed: {e}"));
                }
            };
            api_time_ms += api_start.elapsed().as_millis() as i64;

            total_usage += llm_result.usage;
            budget.record_usage(&llm_result.usage);
            cost_tracker.record(
                &llm_result.model,
                llm_result.usage,
                /*cost_usd*/ 0.0,
                llm_result.total_duration_ms,
            );

            // Extract text and tool calls from response
            let mut response_text = String::new();
            let mut tool_calls: Vec<ToolCallPart> = Vec::new();

            for part in &llm_result.content {
                match part {
                    AssistantContentPart::Text(t) => {
                        response_text.push_str(&t.text);
                        let _delivered = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::TextDelta {
                                turn_id: turn_id.clone(),
                                delta: t.text.clone(),
                            },
                        )
                        .await;
                    }
                    AssistantContentPart::ToolCall(tc) => {
                        tool_calls.push(tc.clone());
                    }
                    AssistantContentPart::Reasoning(r) => {
                        let _delivered = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::ThinkingDelta {
                                turn_id: turn_id.clone(),
                                delta: r.text.clone(),
                            },
                        )
                        .await;
                    }
                    _ => {}
                }
            }

            // Add assistant message to history
            let assistant_msg = Message::Assistant(coco_types::AssistantMessage {
                message: LlmMessage::Assistant {
                    content: llm_result
                        .content
                        .into_iter()
                        .map(convert_to_assistant_content)
                        .collect(),
                    provider_options: None,
                },
                uuid: uuid::Uuid::new_v4(),
                model: llm_result.model.clone(),
                stop_reason: llm_result
                    .stop_reason
                    .as_deref()
                    .and_then(parse_stop_reason),
                usage: Some(llm_result.usage),
                cost_usd: None,
                request_id: llm_result.request_id.clone(),
                api_error: None,
            });
            history.push(assistant_msg);

            // If no tool calls, we're done
            if tool_calls.is_empty() {
                info!(turn, "no tool calls, conversation complete");
                return Ok(make_result(
                    response_text,
                    turn,
                    total_usage,
                    cost_tracker,
                    /*cancelled*/ false,
                    /*budget_exhausted*/ false,
                    last_continue_reason,
                    start_time,
                    api_time_ms,
                    Some("end_turn".into()),
                    permission_denials,
                    history.messages.clone(),
                ));
            }

            // Execute tool calls via StreamingToolExecutor (batch partitioning)
            info!(turn, tool_count = tool_calls.len(), "executing tool calls");
            let mut ctx = self.create_tool_context();
            ctx.user_message_id = Some(user_msg_uuid.clone());

            // Phase 1: Permission checks + build PendingToolCalls
            let mut pending: Vec<PendingToolCall> = Vec::new();
            for tc in &tool_calls {
                let tool_id: ToolId = tc
                    .tool_name
                    .parse()
                    .unwrap_or_else(|_| ToolId::Custom(tc.tool_name.clone()));

                if let Some(tool) = self.tools.get(&tool_id) {
                    let decision = tool.check_permissions(&tc.input, &ctx).await;
                    match decision {
                        PermissionDecision::Deny { message, .. } => {
                            warn!(tool = tc.tool_name, %message, "tool permission denied");
                            // Accumulate the denial for the session result.
                            // TS: QueryEngine.permissionDenials.push(...) wrapper
                            // around canUseTool() in QueryEngine.ts:244-271.
                            permission_denials.push(coco_types::PermissionDenialInfo {
                                tool_name: tc.tool_name.clone(),
                                tool_use_id: tc.tool_call_id.clone(),
                                tool_input: tc.input.clone(),
                            });
                            history.push(make_tool_error_message(
                                &tc.tool_call_id,
                                &tc.tool_name,
                                &tool_id,
                                &format!("Permission denied: {message}"),
                            ));
                            continue;
                        }
                        PermissionDecision::Ask { .. } => {
                            // Route the ask to the permission bridge if one
                            // is installed (e.g. `SdkPermissionBridge` issuing
                            // `approval/askForApproval` to the SDK client).
                            // Fall back to the previous auto-allow behavior
                            // if no bridge is configured — tests and headless
                            // CLI mode still work unchanged.
                            //
                            // TS reference: notifySessionStateChanged(
                            //     'requires_action') in print.ts:818 on
                            // can_use_tool entry, then transition back to
                            // 'running' after the approval resolves.
                            state_tracker
                                .transition_to(coco_types::SessionState::RequiresAction, &event_tx)
                                .await;

                            if let Some(bridge) = self.permission_bridge.as_ref() {
                                // `id` is a fresh correlation id for this
                                // approval request; `tool_use_id` is the
                                // model-assigned tool-call id that the SDK
                                // client uses to group the approval UI with
                                // the tool-call rendering.
                                let request = coco_tool::ToolPermissionRequest {
                                    id: format!("approval-{}", uuid::Uuid::new_v4()),
                                    tool_use_id: tc.tool_call_id.clone(),
                                    agent_id: self.config.session_id.clone(),
                                    tool_name: tc.tool_name.clone(),
                                    description: format!("Approval required for {}", tc.tool_name),
                                    input: tc.input.clone(),
                                };
                                // Make the bridge await cancellation-aware:
                                // if the turn is interrupted while waiting for
                                // the SDK client's approval response, the
                                // oneshot inside `send_server_request` isn't
                                // cancel-aware and would otherwise hang the
                                // engine indefinitely. `select!` lets the
                                // cancel token abort the await and treat it
                                // as a rejection with feedback (same path as
                                // an infrastructure error).
                                let bridge_result = tokio::select! {
                                    biased;
                                    _ = self.cancel.cancelled() => {
                                        Err("Turn cancelled while waiting for \
                                             permission approval".to_string())
                                    }
                                    r = bridge.request_permission(request) => r,
                                };
                                match bridge_result {
                                    Ok(resolution) => match resolution.decision {
                                        coco_tool::ToolPermissionDecision::Rejected => {
                                            let feedback =
                                                resolution.feedback.unwrap_or_else(|| {
                                                    "Permission denied by client".into()
                                                });
                                            warn!(tool = tc.tool_name, "approval bridge: rejected");
                                            permission_denials.push(
                                                coco_types::PermissionDenialInfo {
                                                    tool_name: tc.tool_name.clone(),
                                                    tool_use_id: tc.tool_call_id.clone(),
                                                    tool_input: tc.input.clone(),
                                                },
                                            );
                                            history.push(make_tool_error_message(
                                                &tc.tool_call_id,
                                                &tc.tool_name,
                                                &tool_id,
                                                &format!("Permission denied: {feedback}"),
                                            ));
                                            state_tracker
                                                .transition_to(
                                                    coco_types::SessionState::Running,
                                                    &event_tx,
                                                )
                                                .await;
                                            continue;
                                        }
                                        coco_tool::ToolPermissionDecision::Approved => {
                                            // fall through to execute
                                        }
                                    },
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            tool = tc.tool_name,
                                            "approval bridge failed; auto-denying"
                                        );
                                        permission_denials.push(coco_types::PermissionDenialInfo {
                                            tool_name: tc.tool_name.clone(),
                                            tool_use_id: tc.tool_call_id.clone(),
                                            tool_input: tc.input.clone(),
                                        });
                                        history.push(make_tool_error_message(
                                            &tc.tool_call_id,
                                            &tc.tool_name,
                                            &tool_id,
                                            &format!("Approval bridge error: {e}"),
                                        ));
                                        state_tracker
                                            .transition_to(
                                                coco_types::SessionState::Running,
                                                &event_tx,
                                            )
                                            .await;
                                        continue;
                                    }
                                }
                            }
                            // Back to running whether we consulted a bridge or
                            // fell through to auto-allow.
                            state_tracker
                                .transition_to(coco_types::SessionState::Running, &event_tx)
                                .await;
                        }
                        PermissionDecision::Allow { .. } => {}
                    }

                    // Pre-tool hook (orchestrated with env injection + aggregation)
                    if let Some(hooks) = &self.hooks {
                        let ctx = self.orchestration_ctx();
                        match orchestration::execute_pre_tool_use(
                            hooks,
                            &ctx,
                            &tc.tool_name,
                            &tc.tool_call_id,
                            &tc.input,
                            hook_tx_opt.as_ref(),
                        )
                        .await
                        {
                            Ok(agg) if agg.is_blocked() => {
                                warn!(
                                    tool = tc.tool_name,
                                    "PreToolUse hook blocked tool execution"
                                );
                                continue;
                            }
                            Ok(_agg) => {
                                // Future: apply agg.updated_input, permission_behavior
                            }
                            Err(e) => {
                                warn!(
                                    error = %e,
                                    tool = tc.tool_name,
                                    "PreToolUse hook failed (non-blocking)"
                                );
                            }
                        }
                    }

                    // Emit stream event: tool queued with complete input.
                    let _delivered = emit_stream(
                        &event_tx,
                        crate::AgentStreamEvent::ToolUseQueued {
                            call_id: tc.tool_call_id.clone(),
                            name: tc.tool_name.clone(),
                            input: tc.input.clone(),
                        },
                    )
                    .await;

                    pending.push(PendingToolCall {
                        tool_use_id: tc.tool_call_id.clone(),
                        tool: tool.clone(),
                        input: tc.input.clone(),
                    });
                } else {
                    warn!(tool = tc.tool_name, "tool not found in registry");
                }
            }

            // Phase 2: Execute via StreamingToolExecutor (concurrent-safe tools
            // run in parallel, non-concurrent tools run sequentially).
            //
            // Emit ToolUseStarted for every pending tool so the TUI can
            // transition queued items to "running" state before execution
            // begins. TS has no distinct event for this — coco-rs adds it for
            // richer display.
            for pc in &pending {
                let tool_name = tool_calls
                    .iter()
                    .find(|tc| tc.tool_call_id == pc.tool_use_id)
                    .map(|tc| tc.tool_name.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let _delivered = emit_stream(
                    &event_tx,
                    crate::AgentStreamEvent::ToolUseStarted {
                        call_id: pc.tool_use_id.clone(),
                        name: tool_name,
                        batch_id: None,
                    },
                )
                .await;
            }

            let executor = StreamingToolExecutor::new();
            let results = executor.execute_all(pending, &ctx).await;

            // Pre-serialize successful outputs once so the stream-emit pass and
            // the history-append pass don't re-serialize the same JSON value.
            let output_strs: Vec<String> = results
                .iter()
                .map(|result| match &result.result {
                    Ok(r) => serde_json::to_string(&r.data).unwrap_or_default(),
                    Err(e) => e.to_string(),
                })
                .collect();

            // Phase 3: Emit stream events in arrival order, then process into history.
            for (result, output) in results.iter().zip(output_strs.iter()) {
                let tool_name = tool_calls
                    .iter()
                    .find(|tc| tc.tool_call_id == result.tool_use_id)
                    .map(|tc| tc.tool_name.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                let _delivered = emit_stream(
                    &event_tx,
                    crate::AgentStreamEvent::ToolUseCompleted {
                        call_id: result.tool_use_id.clone(),
                        name: tool_name,
                        output: output.clone(),
                        is_error: result.result.is_err(),
                    },
                )
                .await;
            }

            for (result, output) in results.into_iter().zip(output_strs.into_iter()) {
                let tool_name = tool_calls
                    .iter()
                    .find(|tc| tc.tool_call_id == result.tool_use_id)
                    .map(|tc| tc.tool_name.as_str())
                    .unwrap_or("unknown");

                match result.result {
                    Ok(tool_result) => {
                        // Post-tool hook (orchestrated)
                        if let Some(hooks) = &self.hooks {
                            let ctx = self.orchestration_ctx();
                            if let Err(e) = orchestration::execute_post_tool_use(
                                hooks,
                                &ctx,
                                tool_name,
                                &result.tool_use_id,
                                &serde_json::Value::Null,
                                &tool_result.data,
                                hook_tx_opt.as_ref(),
                            )
                            .await
                            {
                                warn!(
                                    error = %e,
                                    tool = tool_name,
                                    "PostToolUse hook failed (non-blocking)"
                                );
                            }
                        }

                        let result_msg = Message::ToolResult(coco_types::ToolResultMessage {
                            uuid: uuid::Uuid::new_v4(),
                            message: LlmMessage::Tool {
                                content: vec![coco_types::ToolContent::ToolResult(
                                    coco_types::ToolResultContent {
                                        tool_call_id: result.tool_use_id.clone(),
                                        tool_name: tool_name.to_string(),
                                        output: ToolResultContent::text(output),
                                        is_error: false,
                                        provider_metadata: None,
                                    },
                                )],
                                provider_options: None,
                            },
                            tool_use_id: result.tool_use_id,
                            tool_id: result.tool_id,
                            is_error: false,
                        });
                        history.push(result_msg);
                    }
                    Err(e) => {
                        // Post-tool failure hook (orchestrated)
                        if let Some(hooks) = &self.hooks {
                            let ctx = self.orchestration_ctx();
                            let _ = hooks
                                .execute_hooks(HookEventType::PostToolUseFailure, Some(tool_name))
                                .await;
                            drop(ctx);
                        }

                        warn!(tool = tool_name, error = %e, "tool execution failed");
                        history.push(make_tool_error_message(
                            &result.tool_use_id,
                            tool_name,
                            &result.tool_id,
                            &format!("Error: {e}"),
                        ));
                    }
                }
            }

            // Drain command queue: inject queued prompts as user messages.
            // Slash commands excluded (processed post-turn). Agent-filtered.
            let queued = self
                .command_queue
                .get_commands_by_max_priority(QueuePriority::Next, None)
                .await;
            if !queued.is_empty() {
                let count = queued.len() as i32;
                let prompts_to_remove: Vec<String> =
                    queued.iter().map(|c| c.prompt.clone()).collect();
                for cmd in queued {
                    let msg = coco_messages::create_user_message(&cmd.prompt);
                    history.push(msg);
                }
                self.command_queue.remove(&prompts_to_remove).await;
                // Report the new queue state after draining.
                let remaining = self.command_queue.len().await as i32;
                let _delivered = emit_protocol(
                    &event_tx,
                    crate::ServerNotification::QueueStateChanged { queued: remaining },
                )
                .await;
                let _ = count; // count retained for future telemetry
            }

            // Drain inbox messages from teammates.
            let inbox_msgs = self.inbox.drain_unconsumed().await;
            if !inbox_msgs.is_empty() {
                let count = inbox_msgs.len() as i32;
                for msg in inbox_msgs {
                    let text = format!(
                        "<teammate-message from=\"{from}\">{content}</teammate-message>",
                        from = msg.from_agent,
                        content = msg.content
                    );
                    history.push(coco_messages::create_user_message(&text));
                }
                // No direct ServerNotification for inbox consumption;
                // silently consumed into history. count retained for future metrics.
                let _ = count;
            }

            last_continue_reason = Some(ContinueReason::NextTurn);

            // Auto-compaction check after each turn (TS-aligned threshold).
            // TS: compactConversation() in QueryEngine — micro-compact first,
            // then full LLM-summarized compact if still over threshold.
            let estimated_tokens = coco_compact::estimate_tokens(&history.messages);
            if coco_compact::should_auto_compact(
                estimated_tokens,
                self.config.context_window,
                self.config.max_output_tokens,
            ) {
                // Micro-compact first to free tokens quickly
                let pre_count = history.messages.len() as i32;
                coco_compact::micro_compact(&mut history.messages, /*keep_recent*/ 10);
                info!("auto micro-compaction triggered");
                let removed = (pre_count - history.messages.len() as i32).max(0);
                let _delivered = emit_protocol(
                    &event_tx,
                    crate::ServerNotification::ContextCompacted(
                        coco_types::ContextCompactedParams {
                            removed_messages: removed,
                            summary_tokens: 0,
                        },
                    ),
                )
                .await;

                // Re-check: if still over threshold, attempt full LLM compact.
                // TS: falls through to compactConversation() when micro isn't enough.
                let post_micro_tokens = coco_compact::estimate_tokens(&history.messages);
                if coco_compact::should_auto_compact(
                    post_micro_tokens,
                    self.config.context_window,
                    self.config.max_output_tokens,
                ) {
                    self.try_full_compact(&mut history, &event_tx).await;
                }
            }

            // Emit turn completed
            let _delivered = emit_protocol(
                &event_tx,
                crate::ServerNotification::TurnCompleted(coco_types::TurnCompletedParams {
                    turn_id: Some(turn_id),
                    usage: llm_result.usage,
                }),
            )
            .await;
            let _ = tool_calls; // has_tool_calls retained for future metrics
        }
    }

    /// Attempt full LLM-summarized compaction.
    ///
    /// TS: `compactConversation()` — snapshot readFileState, clear it, call LLM
    /// to summarize old rounds, then re-inject recently read files.
    async fn try_full_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    ) {
        // 1. Snapshot + clear FileReadState (TS: cacheToObject + readFileState.clear())
        let snapshot = if let Some(frs) = &self.file_read_state {
            let mut frs = frs.write().await;
            let snap = frs.snapshot_by_recency();
            frs.clear();
            snap
        } else {
            Vec::new()
        };
        // Keep a copy for restoration on failure.
        let snapshot_backup = snapshot.clone();

        // 2. Build the attachment callback that captures the snapshot.
        // TS: createPostCompactFileAttachments + createPlanAttachmentIfNeeded
        let cwd = std::env::current_dir().unwrap_or_default();
        let session_id = self.config.session_id.clone();
        let config_home = self.config_home.clone();
        let attachment_fn: coco_compact::compact::PostCompactAttachmentFn = Box::new(
            move |result: &coco_compact::CompactResult| {
                // Resolve plan file path for exclusion from file restore.
                let plan_file = config_home.as_ref().map(|ch| {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch, /*project_dir*/ None, /*setting*/ None,
                    );
                    coco_context::get_plan_file_path(
                        &session_id,
                        &plans_dir,
                        /*agent_id*/ None,
                    )
                });

                let mut atts = coco_compact::create_post_compact_file_attachments(
                    &snapshot,
                    &result.messages_to_keep,
                    &cwd,
                    plan_file.as_deref(),
                );

                // TS: createPlanAttachmentIfNeeded() — re-inject plan if it exists.
                if let Some(ref ch) = config_home {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch, /*project_dir*/ None, /*setting*/ None,
                    );
                    if let Some(plan_content) =
                        coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None)
                    {
                        let plan_path = coco_context::get_plan_file_path(
                            &session_id,
                            &plans_dir,
                            /*agent_id*/ None,
                        );
                        let text = format!(
                            "A plan file exists from plan mode at: {path}\n\nPlan contents:\n\n{plan_content}",
                            path = plan_path.display(),
                        );
                        atts.push(coco_types::AttachmentMessage {
                            uuid: uuid::Uuid::new_v4(),
                            message: LlmMessage::user_text(
                                coco_messages::wrapping::wrap_in_system_reminder(&text),
                            ),
                            is_meta: true,
                        });
                    }
                }

                atts
            },
        );

        // 3. Build compact config
        let compact_config = coco_compact::CompactConfig {
            context_window: self.config.context_window,
            trigger: coco_types::CompactTrigger::Auto,
            ..Default::default()
        };

        // 4. Call compact_conversation with LLM summarize callback
        let client = self.client.clone();
        let summarize_fn = |prompt: String| {
            let client = client.clone();
            async move {
                let params = QueryParams {
                    prompt: vec![LlmMessage::user_text(&prompt)],
                    max_tokens: Some(coco_compact::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY),
                    thinking_level: None,
                    fast_mode: false,
                    tools: None,
                };
                match client.query(&params).await {
                    Ok(result) => {
                        let text = result
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                AssistantContent::Text(t) => Some(t.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        Ok(text)
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
        };

        match coco_compact::compact_conversation(
            &history.messages,
            &compact_config,
            summarize_fn,
            Some(attachment_fn),
        )
        .await
        {
            Ok(result) => {
                info!(
                    pre = result.pre_compact_tokens,
                    post = result.post_compact_tokens,
                    "full compaction completed"
                );

                // Replace history with TS-aligned order:
                // boundary, summaryMessages, messagesToKeep, attachments, hookResults
                // TS: buildPostCompactMessages() in compact.ts
                let mut new_messages = Vec::new();
                new_messages.push(result.boundary_marker);
                new_messages.extend(result.summary_messages);
                new_messages.extend(result.messages_to_keep);
                for att in &result.attachments {
                    new_messages.push(Message::Attachment(att.clone()));
                }
                new_messages.extend(result.hook_results);
                history.messages = new_messages;

                let _delivered = emit_protocol(
                    event_tx,
                    crate::ServerNotification::ContextCompacted(
                        coco_types::ContextCompactedParams {
                            removed_messages: 0,
                            summary_tokens: result.post_compact_tokens as i32,
                        },
                    ),
                )
                .await;
            }
            Err(e) => {
                warn!("full compaction failed: {e}");
                // Restore FileReadState from backup so dedup/changed-file
                // detection continues to work after a failed compact attempt.
                if let Some(frs) = &self.file_read_state {
                    let mut frs = frs.write().await;
                    for (path, entry) in snapshot_backup {
                        frs.set(path, entry);
                    }
                }
            }
        }
    }

    /// Consume `HookExecutionEvent` from the orchestration layer and forward
    /// them as `CoreEvent::Protocol(HookStarted/Progress/Response)`.
    ///
    /// TS: print.ts emits these directly from the hook execution path; in
    /// Rust we use a child task so orchestration stays independent of
    /// the coco-query event type.
    ///
    /// Graceful shutdown: the normal exit path is for the caller to drop
    /// the matching sender, which makes `rx.recv()` return `None` and
    /// drains any queued events before returning. The `cancel` token is
    /// a fast-path escape hatch for crash scenarios (e.g. the drain
    /// timeout in `run_internal_with_messages` has expired); when
    /// cancelled, pending events are discarded. See plan file WS-5.
    async fn forward_hook_events(
        mut rx: tokio::sync::mpsc::Receiver<coco_hooks::HookExecutionEvent>,
        core_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        cancel: CancellationToken,
    ) {
        let Some(core_tx) = core_tx else {
            return;
        };
        loop {
            let evt = tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(evt) => evt,
                    None => break,
                },
            };
            let notif = match evt {
                coco_hooks::HookExecutionEvent::Started {
                    hook_id,
                    hook_name,
                    hook_event,
                } => crate::ServerNotification::HookStarted(coco_types::HookStartedParams {
                    hook_id,
                    hook_name,
                    hook_event,
                }),
                coco_hooks::HookExecutionEvent::Progress {
                    hook_id,
                    hook_name,
                    stdout,
                    stderr,
                } => crate::ServerNotification::HookProgress(coco_types::HookProgressParams {
                    hook_id,
                    hook_name,
                    // The orchestration-layer event doesn't carry the
                    // hook event name on Progress; consumers can correlate
                    // via `hook_id` against the preceding Started event.
                    hook_event: String::new(),
                    stdout,
                    stderr,
                    output: String::new(),
                }),
                coco_hooks::HookExecutionEvent::Response {
                    hook_id,
                    hook_name,
                    exit_code,
                    stdout,
                    stderr,
                    outcome,
                } => crate::ServerNotification::HookResponse(coco_types::HookResponseParams {
                    hook_id,
                    hook_name,
                    hook_event: String::new(),
                    // orchestration layer merges stdout into output on
                    // the raw event; expose both fields separately for
                    // SDK consumers.
                    output: stdout.clone(),
                    stdout,
                    stderr,
                    exit_code,
                    outcome: hook_outcome_to_status(outcome),
                }),
            };
            if !emit_protocol_owned(&core_tx, notif).await {
                break;
            }
        }
    }

    /// Build an orchestration context from the engine's config.
    fn orchestration_ctx(&self) -> OrchestrationContext {
        OrchestrationContext {
            session_id: self.config.session_id.clone(),
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: self.config.project_dir.clone(),
            permission_mode: Some(format!("{:?}", self.config.permission_mode)),
            cancel: self.cancel.clone(),
            disable_all_hooks: self.config.disable_all_hooks,
            allow_managed_hooks_only: self.config.allow_managed_hooks_only,
        }
    }

    /// Build the LLM prompt from message history.
    fn build_prompt(&self, history: &MessageHistory) -> Vec<LlmMessage> {
        let mut prompt = Vec::new();

        // System prompt: use explicit config or build from CLAUDE.md discovery
        let system_text = if let Some(ref sys) = self.config.system_prompt {
            sys.clone()
        } else {
            let mut text =
                String::from("You are coco, an AI coding assistant. Be concise and helpful.\n\n");
            let cwd = std::env::current_dir().unwrap_or_default();
            let claude_files = coco_context::discover_claude_md_files(&cwd);
            for f in &claude_files {
                text.push_str(&format!("# {}\n{}\n\n", f.path.display(), f.content));
            }
            text
        };
        prompt.push(LlmMessage::system(&system_text));

        // Convert history to LlmMessages
        let normalized = coco_messages::normalize_messages_for_api(&history.messages);
        prompt.extend(normalized);

        prompt
    }

    /// Build tool definitions for the LLM (function tool schemas).
    fn build_tool_definitions(&self) -> Vec<vercel_ai_provider::LanguageModelV4Tool> {
        self.tools
            .loaded_tools()
            .iter()
            .map(|tool| {
                let schema = tool.input_schema();
                let json_schema = tool
                    .input_json_schema()
                    .unwrap_or_else(|| serde_json::to_value(&schema).unwrap_or_default());
                LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
                    name: tool.name().to_string(),
                    description: Some(tool.description(
                        &serde_json::Value::Null,
                        &coco_tool::DescriptionOptions::default(),
                    )),
                    input_schema: json_schema,
                    input_examples: None,
                    strict: None,
                    provider_options: None,
                })
            })
            .collect()
    }

    /// Create tool execution context from engine config.
    fn create_tool_context(&self) -> ToolUseContext {
        ToolUseContext {
            tools: self.tools.clone(),
            main_loop_model: self.config.model_name.clone(),
            thinking_level: None,
            is_non_interactive: false,
            max_budget_usd: None,
            custom_system_prompt: None,
            append_system_prompt: None,
            debug: false,
            verbose: false,
            is_teammate: false,
            cancel: self.cancel.clone(),
            messages: Arc::new(RwLock::new(Vec::new())),
            permission_context: coco_types::ToolPermissionContext {
                mode: self.config.permission_mode,
                additional_dirs: std::collections::HashMap::new(),
                allow_rules: std::collections::HashMap::new(),
                deny_rules: std::collections::HashMap::new(),
                ask_rules: std::collections::HashMap::new(),
                bypass_available: self.config.permission_mode == PermissionMode::BypassPermissions,
                pre_plan_mode: None,
                stripped_dangerous_rules: None,
            },
            tool_use_id: None,
            user_message_id: None,
            agent_id: None,
            agent_type: None,
            file_reading_limits: Default::default(),
            glob_limits: Default::default(),
            nested_memory_attachment_triggers: Arc::new(RwLock::new(Default::default())),
            loaded_nested_memory_paths: Default::default(),
            dynamic_skill_dir_triggers: Arc::new(RwLock::new(Default::default())),
            discovered_skill_names: Default::default(),
            tool_decisions: Default::default(),
            user_modified: false,
            require_can_use_tool: false,
            preserve_tool_use_results: false,
            rendered_system_prompt: None,
            critical_system_reminder: None,
            in_progress_tool_use_ids: Arc::new(RwLock::new(Default::default())),
            side_query: Arc::new(coco_tool::NoOpSideQuery),
            mcp: Arc::new(coco_tool::NoOpMcpHandle),
            schedules: Arc::new(coco_tool::NoOpScheduleStore),
            agent: Arc::new(coco_tool::NoOpAgentHandle),
            cwd_override: None,
            permission_bridge: self.permission_bridge.clone(),
            progress_tx: None,
            task_handle: None,
            // TODO(B1.3 follow-up): bridge app/query hook registry into
            // HookHandle impl to wire PreToolUse/PostToolUse hooks through
            // the executor. For now the executor treats None as a no-op.
            hook_handle: None,
            file_read_state: self.file_read_state.clone(),
            file_history: self.file_history.clone(),
            config_home: self.config_home.clone(),
            session_id_for_history: Some(self.config.session_id.clone()),
            app_state: None,
            local_denial_tracking: None,
            query_chain_id: None,
            query_depth: 0,
        }
    }
}

/// Convert vercel-ai AssistantContentPart → coco_types AssistantContent.
/// These are the same type (re-exported through coco-types).
fn convert_to_assistant_content(part: AssistantContentPart) -> AssistantContent {
    part
}

fn parse_stop_reason(s: &str) -> Option<coco_types::StopReason> {
    match s {
        "stop" => Some(coco_types::StopReason::EndTurn),
        "length" => Some(coco_types::StopReason::MaxTokens),
        "tool-calls" => Some(coco_types::StopReason::ToolUse),
        _ => None,
    }
}

/// Map `HookOutcome` to the protocol-layer `HookOutcomeStatus`.
/// Treats Blocking as Error since blocking is a user-visible failure from the
/// SDK consumer's perspective.
fn hook_outcome_to_status(outcome: coco_types::HookOutcome) -> coco_types::HookOutcomeStatus {
    match outcome {
        coco_types::HookOutcome::Success => coco_types::HookOutcomeStatus::Success,
        coco_types::HookOutcome::Blocking => coco_types::HookOutcomeStatus::Error,
        coco_types::HookOutcome::NonBlockingError => coco_types::HookOutcomeStatus::Error,
        coco_types::HookOutcome::Cancelled => coco_types::HookOutcomeStatus::Cancelled,
    }
}

/// Extract the last assistant text from message history.
fn extract_last_assistant_text(history: &MessageHistory) -> String {
    history
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant(a) => match &a.message {
                LlmMessage::Assistant { content, .. } => content.iter().find_map(|c| {
                    if let AssistantContent::Text(t) = c {
                        Some(t.text.clone())
                    } else {
                        None
                    }
                }),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_default()
}

/// Build a tool error message for history.
fn make_tool_error_message(
    tool_call_id: &str,
    tool_name: &str,
    tool_id: &ToolId,
    message: &str,
) -> Message {
    Message::ToolResult(coco_types::ToolResultMessage {
        uuid: uuid::Uuid::new_v4(),
        message: LlmMessage::Tool {
            content: vec![coco_types::ToolContent::ToolResult(
                coco_types::ToolResultContent {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output: ToolResultContent::error_text(message.to_string()),
                    is_error: true,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        tool_use_id: tool_call_id.to_string(),
        tool_id: tool_id.clone(),
        is_error: true,
    })
}

#[cfg(test)]
#[path = "engine.test.rs"]
mod tests;
