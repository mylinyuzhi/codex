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
use coco_inference::StreamEvent;
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
use coco_types::TokenUsage;
use coco_types::ToolAppState;
use coco_types::ToolId;

use crate::helpers::budget_pct_used;
use crate::helpers::convert_to_assistant_content;
use crate::helpers::drain_command_queue_into_history;
use crate::helpers::extract_last_assistant_text;
use crate::helpers::hook_outcome_to_status;
use crate::helpers::make_tool_error_message;
use crate::helpers::parse_stop_reason;
use crate::helpers::should_continue_for_budget;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::ReasoningPart;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::language_model::v4::LanguageModelV4FunctionTool;

pub use crate::config::ContinueReason;
use crate::config::ESCALATED_MAX_TOKENS;
use crate::config::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT;
pub use crate::config::QueryEngineConfig;
pub use crate::config::QueryResult;
pub use crate::config::SessionBootstrap;

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
    /// Auto-mode state + rules for the 2-stage LLM classifier. When active,
    /// tool calls that return `PermissionDecision::Ask` are first run through
    /// `can_use_tool_in_auto_mode` — Allow/Deny short-circuits the permission
    /// bridge; None falls through to interactive approval. TS: classifier flow
    /// in `utils/permissions/classifierDecision.ts`.
    auto_mode_state: Option<Arc<coco_permissions::AutoModeState>>,
    denial_tracker: Option<Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>>,
    auto_mode_rules: coco_permissions::AutoModeRules,
    /// Shared cross-turn app state (typed) — carries flags like
    /// `needs_plan_mode_exit_attachment` set by `ExitPlanModeTool`.
    /// Attached via [`Self::with_app_state`]; absent on engines that
    /// don't need this signalling.
    app_state: Option<Arc<RwLock<ToolAppState>>>,
    /// Mailbox handle for swarm teammate messaging. `None` resolves to
    /// `NoOpMailboxHandle` in `create_tool_context`; swarm spawn paths
    /// install a real handle via [`Self::with_mailbox`].
    mailbox: Option<coco_tool::MailboxHandleRef>,
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
            auto_mode_state: None,
            denial_tracker: None,
            auto_mode_rules: coco_permissions::AutoModeRules::default(),
            app_state: None,
            mailbox: None,
        }
    }

    /// Install a mailbox handle for swarm teammate messaging.
    pub fn with_mailbox(mut self, mailbox: coco_tool::MailboxHandleRef) -> Self {
        self.mailbox = Some(mailbox);
        self
    }

    /// Attach auto-mode state + rules so `PermissionDecision::Ask` outcomes
    /// are first classified by the 2-stage LLM sidequery before falling back
    /// to interactive approval.
    pub fn with_auto_mode(
        mut self,
        state: Arc<coco_permissions::AutoModeState>,
        denial_tracker: Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>,
        rules: coco_permissions::AutoModeRules,
    ) -> Self {
        self.auto_mode_state = Some(state);
        self.denial_tracker = Some(denial_tracker);
        self.auto_mode_rules = rules;
        self
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

    /// Attach a shared `ToolAppState` for cross-component signalling.
    ///
    /// Tools read/write this via `ToolUseContext.app_state` — plan mode's
    /// exit flag, plan-file entry timestamp, and the live permission
    /// mode (`permission_mode`, `pre_plan_mode`, `stripped_dangerous_rules`)
    /// are carried here. Without this the engine runs normally but
    /// the plan-mode-exit reminder never fires and tool mode changes
    /// don't propagate across LLM iterations.
    ///
    /// **Bootstrap**: if `app_state.permission_mode` is `None` (fresh
    /// state), it's seeded from `self.config.permission_mode` so the
    /// first batch's `create_tool_context` sees a concrete mode. If
    /// already `Some(_)` (e.g. session resumed, prior-run state
    /// carried), the existing value is preserved — user + tool
    /// intent trumps config. TS parity: `appState` is
    /// initialized-once at session-create and never re-seeded from
    /// config afterward.
    pub fn with_app_state(mut self, app_state: Arc<RwLock<ToolAppState>>) -> Self {
        // Bootstrap the live mode on first attach. This is a one-shot
        // write — subsequent runs that reuse the same app_state see
        // the preserved value rather than an overwrite.
        if let Ok(mut guard) = app_state.try_write() {
            if guard.permission_mode.is_none() {
                guard.permission_mode = Some(self.config.permission_mode);
            }
        }
        self.app_state = Some(app_state);
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

    /// Set the config home directory. Used by plan-mode (`plans_dir`
    /// resolution) and surfaced on `ToolUseContext.config_home` so
    /// tools can locate the plan file on disk. `with_file_history`
    /// also sets this as a side-effect; use this builder when you
    /// need config_home without attaching a file-history state (e.g.
    /// integration tests).
    pub fn with_config_home(mut self, config_home: std::path::PathBuf) -> Self {
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
        // max-output-tokens recovery state (TS: query.ts State.maxOutputTokensOverride + maxOutputTokensRecoveryCount)
        let mut max_tokens_override: Option<i64> = None;
        let mut max_tokens_recovery_count: i32 = 0;
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

        // Plan-mode reminder tracker — injects the system-reminder at the
        // start of every turn while plan mode is active and on the turn
        // following an ExitPlanMode approval. TS: normalizeAttachmentForAPI
        // cases `plan_mode` / `plan_mode_exit` / `plan_mode_reentry`.
        let plans_dir = crate::plan_mode_reminder::PlanModeReminder::resolve_plans_dir(
            self.config_home.as_deref(),
            self.config.project_dir.as_deref(),
            self.config.plans_directory.as_deref(),
        );
        let pm = &self.config.plan_mode_settings;
        let workflow = match pm.workflow {
            coco_config::PlanModeWorkflow::FivePhase => coco_context::PlanWorkflow::FivePhase,
            coco_config::PlanModeWorkflow::Interview => coco_context::PlanWorkflow::Interview,
        };
        let phase4 = match pm.phase4_variant {
            coco_config::PlanPhase4Variant::Standard => coco_context::Phase4Variant::Standard,
            coco_config::PlanPhase4Variant::Trim => coco_context::Phase4Variant::Trim,
            coco_config::PlanPhase4Variant::Cut => coco_context::Phase4Variant::Cut,
            coco_config::PlanPhase4Variant::Cap => coco_context::Phase4Variant::Cap,
        };
        let mut plan_reminder = crate::plan_mode_reminder::PlanModeReminder::new(
            self.config.permission_mode,
            Some(self.config.session_id.clone()),
            self.config.agent_id.clone(),
            plans_dir,
            self.app_state.clone(),
        )
        .with_workflow(workflow)
        .with_phase4_variant(phase4)
        .with_agent_counts(pm.explore_agent_count, pm.plan_agent_count);
        // Wire mailbox for swarm polling if identity is set and a mailbox
        // handle is installed. Agent + team names come from env vars
        // (set by the swarm spawner); mirror `swarm_identity::get_agent_name`
        // env fallback. We keep the env read here rather than threading
        // via ctx because the reminder is engine-level (no ToolUseContext).
        // Env namespace is `COCO_*` — see swarm_constants.
        let agent_name_env = std::env::var("COCO_AGENT_NAME").ok();
        let team_name_env = std::env::var("COCO_TEAM_NAME").ok();
        if let (Some(mbox), Some(agent), Some(team)) =
            (self.mailbox.clone(), agent_name_env, team_name_env)
        {
            plan_reminder = plan_reminder.with_mailbox(
                mbox,
                agent,
                team,
                self.config.is_teammate && self.config.plan_mode_required,
            );
        }

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

            // Inject plan-mode / plan-mode-exit reminder before building
            // the LLM prompt. No-op when the engine isn't in plan mode
            // and the exit flag isn't set.
            plan_reminder.turn_start(&mut history).await;

            // Build prompt from history
            let prompt = self.build_prompt(&history);
            let tool_defs = self.build_tool_definitions();

            // StreamRequestStart has no direct protocol equivalent; it was
            // previously only used for test classification. The model_name is
            // already carried in SessionStarted at session init.

            // Call LLM via streaming. TextDelta/ThinkingDelta events fire
            // as the model generates, not post-hoc — so SDK consumers and the
            // TUI see tokens land in real-time. Tool calls are accumulated
            // into ordered buffers and dispatched after the stream finishes
            // (mid-stream tool dispatch is a follow-up — see PR-E1 Phase 2).
            //
            // TS reference: query.ts:659-845 (streaming loop + tool exec).
            // Escalation takes the MAX of the override and the user config so
            // we never DOWNGRADE a user-configured higher limit (e.g. user
            // set 128k, override says 64k → keep 128k, already sufficient).
            let effective_max_tokens = match (max_tokens_override, self.config.max_tokens) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(v), None) | (None, Some(v)) => Some(v),
                (None, None) => None,
            };
            let params = QueryParams {
                prompt,
                max_tokens: effective_max_tokens,
                thinking_level: None,
                fast_mode: false,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
            };

            let api_start = std::time::Instant::now();
            let mut rx = match self.client.query_stream(&params).await {
                Ok(rx) => rx,
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                        warn!("prompt too long (stream open), attempting reactive compaction");
                        self.do_reactive_compact(&mut history, &event_tx).await;
                        last_continue_reason = Some(ContinueReason::ReactiveCompactRetry);
                        budget.reset_continuations();
                        continue;
                    }
                    return Err(anyhow::anyhow!("LLM stream open failed: {e}"));
                }
            };

            // Accumulate stream state. `tool_order` preserves the order tool
            // calls first appeared (by `ToolInputStart`) so the downstream
            // exec path keeps the same ordering contract as the blocking path.
            //
            // `early_handles` carries concurrency-safe tool executions that
            // were dispatched mid-stream (see `try_eager_dispatch`). The
            // post-stream tool-exec loop awaits these before running the
            // batch executor for the remaining tools. This is PR-E1 Phase 2:
            // overlap tool execution with API streaming for hook-free
            // sessions. TS: `query.ts:710-845` dispatches into
            // `StreamingToolExecutor` as tool_use blocks arrive.
            let mut response_text = String::new();
            let mut reasoning_text = String::new();
            let mut tool_order: Vec<String> = Vec::new();
            let mut tool_buffers: std::collections::HashMap<String, StreamingToolCallBuffer> =
                std::collections::HashMap::new();
            let mut stream_usage: Option<TokenUsage> = None;
            let mut stream_stop_reason: Option<String> = None;
            let mut stream_error: Option<String> = None;

            // Eager-dispatch context: shared across all tasks spawned from
            // within the stream loop. Hook-free sessions get parallelism;
            // hook-configured sessions fall through to the existing batch
            // path to keep PreToolUse/PostToolUse ordering intact.
            let mut stream_ctx_owned = self.create_tool_context().await;
            stream_ctx_owned.user_message_id = Some(user_msg_uuid.clone());
            let stream_ctx: Arc<ToolUseContext> = Arc::new(stream_ctx_owned.clone_for_concurrent());
            let mut early_handles: std::collections::HashMap<
                String,
                tokio::task::JoinHandle<
                    Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError>,
                >,
            > = std::collections::HashMap::new();
            let eager_enabled = self.hooks.is_none();

            loop {
                let event = tokio::select! {
                    _ = self.cancel.cancelled() => {
                        drop(rx);
                        return Err(anyhow::anyhow!("query cancelled during stream"));
                    }
                    ev = rx.recv() => ev,
                };
                let Some(event) = event else {
                    // Channel closed without Finish/Error — treat as a premature
                    // end. Keep whatever content we accumulated; callers fall
                    // through to the empty-tool_calls exit below.
                    break;
                };

                match event {
                    StreamEvent::TextDelta { text } => {
                        response_text.push_str(&text);
                        let _ = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::TextDelta {
                                turn_id: turn_id.clone(),
                                delta: text,
                            },
                        )
                        .await;
                    }
                    StreamEvent::ReasoningDelta { text } => {
                        reasoning_text.push_str(&text);
                        let _ = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::ThinkingDelta {
                                turn_id: turn_id.clone(),
                                delta: text,
                            },
                        )
                        .await;
                    }
                    StreamEvent::ToolCallStart { id, tool_name } => {
                        if !tool_buffers.contains_key(&id) {
                            tool_order.push(id.clone());
                        }
                        tool_buffers.insert(
                            id.clone(),
                            StreamingToolCallBuffer {
                                tool_name,
                                input_json: String::new(),
                                complete: false,
                            },
                        );
                    }
                    StreamEvent::ToolCallDelta { id, delta } => {
                        if let Some(buf) = tool_buffers.get_mut(&id) {
                            buf.input_json.push_str(&delta);
                        }
                    }
                    StreamEvent::ToolCallEnd { id } => {
                        if let Some(buf) = tool_buffers.get_mut(&id) {
                            buf.complete = true;
                        }
                        // PR-E1 Phase 2: try to dispatch this tool mid-stream
                        // so safe read-only tools overlap with the API stream.
                        if eager_enabled
                            && let Some(buf) = tool_buffers.get(&id)
                            && buf.complete
                        {
                            let input_result: Result<serde_json::Value, _> =
                                if buf.input_json.trim().is_empty() {
                                    Ok(serde_json::Value::Object(Default::default()))
                                } else {
                                    serde_json::from_str(&buf.input_json)
                                };
                            if let Ok(input) = input_result {
                                let tool_name = buf.tool_name.clone();
                                let tool_id: ToolId = tool_name
                                    .parse()
                                    .unwrap_or_else(|_| ToolId::Custom(tool_name.clone()));
                                if let Some(tool) = self.tools.get(&tool_id).cloned()
                                    && tool.is_concurrency_safe(&input)
                                {
                                    let decision =
                                        tool.check_permissions(&input, &stream_ctx).await;
                                    if let PermissionDecision::Allow { .. } = decision {
                                        // Emit Queued + Started now so the
                                        // consumer sees the lifecycle begin
                                        // during the stream.
                                        let _ = emit_stream(
                                            &event_tx,
                                            crate::AgentStreamEvent::ToolUseQueued {
                                                call_id: id.clone(),
                                                name: tool_name.clone(),
                                                input: input.clone(),
                                            },
                                        )
                                        .await;
                                        let _ = emit_stream(
                                            &event_tx,
                                            crate::AgentStreamEvent::ToolUseStarted {
                                                call_id: id.clone(),
                                                name: tool_name.clone(),
                                                batch_id: None,
                                            },
                                        )
                                        .await;
                                        let ctx_arc = stream_ctx.clone();
                                        let input_clone = input.clone();
                                        let handle = tokio::spawn(async move {
                                            tool.execute(input_clone, &ctx_arc).await
                                        });
                                        early_handles.insert(id.clone(), handle);
                                    }
                                }
                            }
                        }
                    }
                    StreamEvent::Finish { usage, stop_reason } => {
                        stream_usage = Some(usage);
                        stream_stop_reason = Some(stop_reason);
                        break;
                    }
                    StreamEvent::Error { message } => {
                        stream_error = Some(message);
                        break;
                    }
                }
            }

            let api_elapsed_ms = api_start.elapsed().as_millis() as i64;
            api_time_ms += api_elapsed_ms;

            if let Some(err_msg) = stream_error {
                if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                    warn!("prompt too long (stream), attempting reactive compaction");
                    self.do_reactive_compact(&mut history, &event_tx).await;
                    last_continue_reason = Some(ContinueReason::ReactiveCompactRetry);
                    budget.reset_continuations();
                    continue;
                }
                return Err(anyhow::anyhow!("LLM stream failed: {err_msg}"));
            }

            let usage = stream_usage.unwrap_or_default();
            total_usage += usage;
            budget.record_usage(&usage);
            let model_id = self.client.model_id().to_string();
            cost_tracker.record(&model_id, usage, /*cost_usd*/ 0.0, api_elapsed_ms);

            // Re-materialize `tool_calls` from buffers in arrival order.
            // Malformed JSON or incomplete buffers are skipped with a warning —
            // matches the blocking path's behavior of silently ignoring
            // AssistantContentPart variants it doesn't recognize.
            let mut tool_calls: Vec<ToolCallPart> = Vec::new();
            for call_id in &tool_order {
                let Some(buf) = tool_buffers.get(call_id) else {
                    continue;
                };
                if !buf.complete {
                    warn!(tool_call_id = %call_id, "tool call buffer did not complete");
                    continue;
                }
                let input: serde_json::Value = if buf.input_json.trim().is_empty() {
                    serde_json::Value::Object(Default::default())
                } else {
                    match serde_json::from_str(&buf.input_json) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(
                                tool_call_id = %call_id,
                                tool_name = %buf.tool_name,
                                error = %e,
                                raw_input = %buf.input_json,
                                "tool input JSON parse failed"
                            );
                            continue;
                        }
                    }
                };
                tool_calls.push(ToolCallPart {
                    tool_call_id: call_id.clone(),
                    tool_name: buf.tool_name.clone(),
                    input,
                    provider_executed: None,
                    provider_metadata: None,
                });
            }

            // Reconstruct the assistant `content` vector: reasoning → text →
            // tool calls. Matches the typical ordering from the blocking
            // `do_generate` path; individual providers may interleave
            // differently, but the stream doesn't preserve relative ordering
            // between text and reasoning chunks anyway.
            let mut content_parts: Vec<AssistantContentPart> = Vec::new();
            if !reasoning_text.is_empty() {
                content_parts.push(AssistantContentPart::Reasoning(ReasoningPart {
                    text: reasoning_text,
                    provider_metadata: None,
                }));
            }
            if !response_text.is_empty() {
                content_parts.push(AssistantContentPart::Text(TextPart {
                    text: response_text.clone(),
                    provider_metadata: None,
                }));
            }
            for tc in &tool_calls {
                content_parts.push(AssistantContentPart::ToolCall(tc.clone()));
            }

            let parsed_stop_reason = stream_stop_reason.as_deref().and_then(parse_stop_reason);
            let assistant_msg = Message::Assistant(coco_types::AssistantMessage {
                message: LlmMessage::Assistant {
                    content: content_parts
                        .into_iter()
                        .map(convert_to_assistant_content)
                        .collect(),
                    provider_options: None,
                },
                uuid: uuid::Uuid::new_v4(),
                model: model_id.clone(),
                stop_reason: parsed_stop_reason,
                usage: Some(usage),
                cost_usd: None,
                request_id: None,
                api_error: None,
            });

            // Max-output-tokens recovery: the model hit `length` stop with no
            // tool calls (otherwise it's mid-call and we proceed normally).
            // Phase 1: escalate `max_output_tokens` to 64k and retry without
            //          persisting the truncated response (TS: query.ts:1199-1221).
            // Phase 2: if already escalated, keep the partial response and
            //          inject a "resume" meta user message (TS: query.ts:1223-1249),
            //          up to MAX_OUTPUT_TOKENS_RECOVERY_LIMIT times.
            if tool_calls.is_empty()
                && parsed_stop_reason == Some(coco_types::StopReason::MaxTokens)
            {
                // Escalation only helps when the user's configured limit is
                // BELOW the escalation target. If they're already >= 64k (or
                // we've already escalated this session), skip straight to
                // recovery. TS: `query.ts:1201-1202` guards on env override.
                let user_already_at_escalated = self
                    .config
                    .max_tokens
                    .is_some_and(|v| v >= ESCALATED_MAX_TOKENS);
                if max_tokens_override.is_none() && !user_already_at_escalated {
                    warn!(
                        escalated_to = ESCALATED_MAX_TOKENS,
                        "max_tokens hit, escalating"
                    );
                    max_tokens_override = Some(ESCALATED_MAX_TOKENS);
                    last_continue_reason = Some(ContinueReason::MaxOutputTokensEscalate);
                    continue;
                } else if max_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT {
                    max_tokens_recovery_count += 1;
                    warn!(
                        attempt = max_tokens_recovery_count,
                        "max_tokens hit after escalation, injecting resume nudge"
                    );
                    history.push(assistant_msg);
                    history.push(coco_messages::create_meta_message(
                        "Output token limit hit. Resume directly — no apology, no recap of \
                         what you were doing. Pick up mid-thought if that is where the cut \
                         happened. Break remaining work into smaller pieces.",
                    ));
                    // Reset override so next call uses the provider default again;
                    // TS does the same (query.ts:1241 `maxOutputTokensOverride: undefined`).
                    max_tokens_override = None;
                    last_continue_reason = Some(ContinueReason::MaxOutputTokensRecovery {
                        attempt: max_tokens_recovery_count,
                    });
                    continue;
                }
                // Recovery exhausted — fall through and terminate the session normally.
            }

            history.push(assistant_msg);

            // If no tool calls, we're done — unless token-budget-continuation
            // is enabled and we're well under budget: inject a nudge and loop.
            // TS: `query.ts:1308-1340` `feature('TOKEN_BUDGET')` path.
            if tool_calls.is_empty() {
                // Stop hooks: let external hooks block session completion and
                // inject feedback into the conversation. If any Stop hook
                // blocks, the loop continues with the feedback visible to the
                // model. TS: `query.ts` `handleStopHooks()` around line 1050.
                if let Some(hooks) = &self.hooks {
                    let hook_ctx = self.orchestration_ctx();
                    match orchestration::execute_stop(
                        hooks,
                        &hook_ctx,
                        Some("end_turn"),
                        hook_tx_opt.as_ref(),
                    )
                    .await
                    {
                        Ok(agg) if agg.is_blocked() => {
                            if let Some(err) = &agg.blocking_error {
                                let feedback = orchestration::format_stop_hook_message(err);
                                warn!(%feedback, "Stop hook blocked session completion");
                                history.push(coco_messages::create_meta_message(&feedback));
                                last_continue_reason = Some(ContinueReason::StopHookBlocking);
                                continue;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => warn!(error = %e, "Stop hook execution failed"),
                    }
                }

                if self.config.enable_token_budget_continuation
                    && should_continue_for_budget(&budget)
                {
                    let pct = budget_pct_used(&budget);
                    let nudge = format!(
                        "Token budget continuation: you've used {pct}% of the turn budget. \
                         Keep going — don't summarize or recap, just continue the work."
                    );
                    history.push(coco_messages::create_meta_message(&nudge));
                    budget.record_continuation();
                    last_continue_reason = Some(ContinueReason::TokenBudgetContinuation);
                    info!(turn, pct, "token budget continuation");
                    continue;
                }
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

            // Mid-turn `Now`-priority drain: urgent user input that arrived
            // during streaming is flushed before we start executing tools, so
            // it's visible on the next API call without waiting for the whole
            // tool batch to complete. Non-Now commands defer to the end-of-turn
            // drain below to preserve tool_use/tool_result pairing in history.
            drain_command_queue_into_history(
                &self.command_queue,
                &mut history,
                &event_tx,
                QueuePriority::Now,
                None,
            )
            .await;

            // Execute tool calls via StreamingToolExecutor (batch partitioning)
            info!(turn, tool_count = tool_calls.len(), "executing tool calls");
            let mut ctx = self.create_tool_context().await;
            ctx.user_message_id = Some(user_msg_uuid.clone());

            // Phase 1: Permission checks + build PendingToolCalls
            let mut pending: Vec<PendingToolCall> = Vec::new();
            for tc in &tool_calls {
                let tool_id: ToolId = tc
                    .tool_name
                    .parse()
                    .unwrap_or_else(|_| ToolId::Custom(tc.tool_name.clone()));

                // PR-E1 Phase 2: if this tool was eagerly dispatched during
                // the stream, await its result and push to history without
                // running the full Phase 1/2/3 pipeline. Queued + Started
                // were already emitted from the stream loop; we only need
                // Completed + history here.
                if let Some(handle) = early_handles.remove(&tc.tool_call_id) {
                    let mut exec_outcome: Result<
                        coco_types::ToolResult<serde_json::Value>,
                        coco_tool::ToolError,
                    > = match handle.await {
                        Ok(res) => res,
                        Err(join_err) => {
                            warn!(
                                tool = tc.tool_name,
                                error = %join_err,
                                "eager tool task join failed"
                            );
                            Err(coco_tool::ToolError::Cancelled)
                        }
                    };
                    // Eager-dispatched tools bypass the executor —
                    // apply their queued `app_state_patch` here so the
                    // shared store sees the mutation. Without this,
                    // any patch returned from a concurrency-safe tool
                    // that was eagerly dispatched during the stream
                    // would be silently dropped. TS parity:
                    // `orchestration.ts` applies `queuedContext
                    // Modifiers` once per batch regardless of when
                    // individual tools actually dispatched.
                    if let (Ok(tr), Some(arc)) = (exec_outcome.as_mut(), self.app_state.as_ref()) {
                        if let Some(patch) = tr.app_state_patch.take() {
                            let mut guard = arc.write().await;
                            patch(&mut guard);
                        }
                    }
                    let output = match &exec_outcome {
                        Ok(r) => serde_json::to_string(&r.data).unwrap_or_default(),
                        Err(e) => e.to_string(),
                    };
                    let _ = emit_stream(
                        &event_tx,
                        crate::AgentStreamEvent::ToolUseCompleted {
                            call_id: tc.tool_call_id.clone(),
                            name: tc.tool_name.clone(),
                            output: output.clone(),
                            is_error: exec_outcome.is_err(),
                        },
                    )
                    .await;
                    match exec_outcome {
                        Ok(_) => {
                            let result_msg = Message::ToolResult(coco_types::ToolResultMessage {
                                uuid: uuid::Uuid::new_v4(),
                                message: LlmMessage::Tool {
                                    content: vec![coco_types::ToolContent::ToolResult(
                                        coco_types::ToolResultContent {
                                            tool_call_id: tc.tool_call_id.clone(),
                                            tool_name: tc.tool_name.clone(),
                                            output: ToolResultContent::text(output),
                                            is_error: false,
                                            provider_metadata: None,
                                        },
                                    )],
                                    provider_options: None,
                                },
                                tool_use_id: tc.tool_call_id.clone(),
                                tool_id: tool_id.clone(),
                                is_error: false,
                            });
                            history.push(result_msg);
                        }
                        Err(e) => {
                            warn!(tool = tc.tool_name, error = %e, "eager tool execution failed");
                            history.push(make_tool_error_message(
                                &tc.tool_call_id,
                                &tc.tool_name,
                                &tool_id,
                                &format!("Error: {e}"),
                            ));
                        }
                    }
                    continue;
                }

                if let Some(tool) = self.tools.get(&tool_id) {
                    let mut decision = tool.check_permissions(&tc.input, &ctx).await;

                    // Auto-mode classifier: for `Ask` outcomes, run the 2-stage
                    // LLM sidequery BEFORE falling through to the interactive
                    // permission bridge. If the classifier allows or blocks,
                    // short-circuit; otherwise (None), drop to the bridge path.
                    // TS: `classifierDecision.ts` `canUseToolInAutoMode()`.
                    if matches!(decision, PermissionDecision::Ask { .. })
                        && let (Some(state), Some(tracker)) =
                            (self.auto_mode_state.as_ref(), self.denial_tracker.as_ref())
                        && state.is_active()
                    {
                        let is_read_only = tool.is_read_only(&tc.input);
                        let mut tracker_guard = tracker.lock().await;
                        let classifier_decision = self
                            .try_classify_in_auto_mode(
                                &tc.tool_name,
                                &tc.input,
                                is_read_only,
                                state,
                                &mut tracker_guard,
                                &history.messages,
                            )
                            .await;
                        drop(tracker_guard);
                        if let Some(d) = classifier_decision {
                            decision = d;
                        }
                    }

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

            // Wire the executor with the engine's write-capable
            // Arc so it can apply `ToolResult::app_state_patch` after
            // each batch. Tools see `ctx.app_state` as a read-only
            // `AppStateReadHandle`; the executor is the only path
            // through which their returned patches reach the shared
            // store. TS parity: the orchestrator owns the "queue and
            // apply post-batch" responsibility.
            let executor = match self.app_state.as_ref() {
                Some(arc) => StreamingToolExecutor::new().with_app_state(arc.clone()),
                None => StreamingToolExecutor::new(),
            };
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

            self.finalize_turn_post_tools(&mut history, &event_tx, turn_id, usage)
                .await;
            last_continue_reason = Some(ContinueReason::NextTurn);
            let _ = tool_calls; // has_tool_calls retained for future metrics
        }
    }

    /// Run the auto-mode 2-stage LLM classifier for a tool call that returned
    /// `Ask`. Returns `Some(decision)` when the classifier decided, or `None`
    /// when the caller should fall through to interactive approval.
    ///
    /// TS: `classifierDecision.ts` `canUseToolInAutoMode()`.
    async fn try_classify_in_auto_mode(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        is_read_only: bool,
        state: &coco_permissions::AutoModeState,
        tracker: &mut coco_permissions::DenialTracker,
        messages: &[Message],
    ) -> Option<PermissionDecision> {
        let client = Arc::clone(&self.client);
        // `classify_fn` runs the 2-stage LLM call. Each stage issues a fresh
        // one-shot request with (system, user) content — no tools, no streaming.
        let classify_fn = move |req: coco_permissions::ClassifyRequest| {
            let client = Arc::clone(&client);
            async move {
                let prompt: vercel_ai_provider::LanguageModelV4Prompt = vec![
                    vercel_ai_provider::LanguageModelV4Message::System {
                        content: req.system_prompt,
                        provider_options: None,
                    },
                    vercel_ai_provider::LanguageModelV4Message::User {
                        content: vec![vercel_ai_provider::UserContentPart::Text(
                            vercel_ai_provider::TextPart {
                                text: req.user_prompt,
                                provider_metadata: None,
                            },
                        )],
                        provider_options: None,
                    },
                ];
                // Stage 1 (256 tokens, triage) benefits from fast mode — lower
                // latency on the hot path. Stage 2 (4k tokens, extended
                // reasoning) needs the full-capability model, so don't force
                // the fast variant there.
                let params = coco_inference::QueryParams {
                    prompt,
                    max_tokens: Some(req.max_tokens),
                    thinking_level: None,
                    fast_mode: req.stage == 1,
                    tools: None,
                };
                match client.query(&params).await {
                    Ok(result) => {
                        let text: String = result
                            .content
                            .iter()
                            .filter_map(|p| match p {
                                vercel_ai_provider::AssistantContentPart::Text(t) => {
                                    Some(t.text.as_str())
                                }
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

        coco_permissions::can_use_tool_in_auto_mode(
            tool_name,
            input,
            is_read_only,
            state,
            tracker,
            messages,
            &self.auto_mode_rules,
            classify_fn,
        )
        .await
    }

    /// Shrink `history` with a reactive microcompact and emit the paired
    /// `CompactionStarted` → `ContextCompacted` notifications. Shared by both
    /// `prompt_too_long` recovery sites (stream-open failure and mid-stream
    /// failure) — keeps the two paths bit-identical.
    async fn do_reactive_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    ) {
        let pre_count = history.messages.len() as i32;
        let drop_target = coco_compact::reactive::calculate_drop_target(
            coco_compact::estimate_tokens(&history.messages),
            &coco_compact::ReactiveCompactConfig {
                context_window: self.config.context_window,
                max_output_tokens: self.config.max_output_tokens,
                ..Default::default()
            },
        );
        let _ = emit_protocol(event_tx, crate::ServerNotification::CompactionStarted).await;
        coco_compact::reactive::api_microcompact(&mut history.messages, drop_target);
        let removed = (pre_count - history.messages.len() as i32).max(0);
        let _ = emit_protocol(
            event_tx,
            crate::ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                removed_messages: removed,
                summary_tokens: 0,
            }),
        )
        .await;
    }

    /// Finalize a turn after tools have executed: drain queued commands + inbox,
    /// auto-compact if over threshold, then emit `TurnCompleted`.
    ///
    /// Extracted from `run_session_loop` to keep that function focused on the
    /// decision/transition logic. Mirrors the TS tail-of-turn sequence in
    /// `query.ts` where messageQueueManager flush + compactConversation +
    /// turn-complete emission all happen together.
    async fn finalize_turn_post_tools(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        turn_id: String,
        usage: TokenUsage,
    ) {
        // Drain command queue: all priorities land before the next API call.
        // Slash commands excluded (processed post-turn). Agent-filtered.
        // TS: `messageQueueManager.ts` flushes pending messages between tool
        // execution and the next API call.
        drain_command_queue_into_history(
            &self.command_queue,
            history,
            event_tx,
            QueuePriority::Later,
            None,
        )
        .await;

        // Drain inbox messages from teammates.
        let inbox_msgs = self.inbox.drain_unconsumed().await;
        for msg in inbox_msgs {
            let text = format!(
                "<teammate-message from=\"{from}\">{content}</teammate-message>",
                from = msg.from_agent,
                content = msg.content
            );
            history.push(coco_messages::create_user_message(&text));
        }

        // Auto-compaction check: micro first, then full LLM if still over.
        // TS: `compactConversation()` — micro-compact, then full summarize.
        let estimated_tokens = coco_compact::estimate_tokens(&history.messages);
        if coco_compact::should_auto_compact(
            estimated_tokens,
            self.config.context_window,
            self.config.max_output_tokens,
        ) {
            let pre_count = history.messages.len() as i32;
            coco_compact::micro_compact(&mut history.messages, /*keep_recent*/ 10);
            info!("auto micro-compaction triggered");
            let removed = (pre_count - history.messages.len() as i32).max(0);
            let _ = emit_protocol(
                event_tx,
                crate::ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                    removed_messages: removed,
                    summary_tokens: 0,
                }),
            )
            .await;

            let post_micro_tokens = coco_compact::estimate_tokens(&history.messages);
            if coco_compact::should_auto_compact(
                post_micro_tokens,
                self.config.context_window,
                self.config.max_output_tokens,
            ) {
                self.try_full_compact(history, event_tx).await;
            }
        }

        let _ = emit_protocol(
            event_tx,
            crate::ServerNotification::TurnCompleted(coco_types::TurnCompletedParams {
                turn_id: Some(turn_id),
                usage,
            }),
        )
        .await;
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
        let project_dir = self.config.project_dir.clone();
        let plans_directory_setting = self.config.plans_directory.clone();
        let attachment_fn: coco_compact::compact::PostCompactAttachmentFn =
            Box::new(move |result: &coco_compact::CompactResult| {
                // Resolve plan file path for exclusion from file restore.
                let plan_file = config_home.as_ref().map(|ch| {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        project_dir.as_deref(),
                        plans_directory_setting.as_deref(),
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

                // TS: `createPlanAttachmentIfNeeded()` — re-inject plan
                // if it exists so it survives the compaction boundary.
                // Wrap the plan body in `<plan>` XML tags so the model
                // can distinguish plan content from ambient context
                // (TS `plan_file_reference` attachment format at
                // `messages.ts:3636-3642`).
                if let Some(ref ch) = config_home {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        project_dir.as_deref(),
                        plans_directory_setting.as_deref(),
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
                            "A plan file exists from plan mode at: {path}\n\n\
                             <plan>\n{plan_content}\n</plan>\n\n\
                             If this plan is relevant to the current work and not \
                             already complete, continue working on it.",
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
            });

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

    /// Create tool execution context from engine config + live
    /// [`ToolAppState`]. The permission-mode-related fields
    /// (`mode`, `pre_plan_mode`, `stripped_dangerous_rules`) are
    /// seeded from `app_state` when present so mutations made by prior
    /// tool batches (e.g. `EnterPlanMode` setting mode → Plan) are
    /// visible on the next batch. TS parity: every tool-side access
    /// goes through `context.getAppState().toolPermissionContext` —
    /// Rust rebuilds the ctx snapshot per batch from the same shared
    /// store to match that semantic.
    ///
    /// `config.permission_mode` is used only as a fallback when
    /// `app_state` is absent (single-shot SDK callers, tests without a
    /// shared state). Callers wiring `with_app_state` are expected to
    /// seed `app_state.permission_mode` from `config.permission_mode`
    /// once at session bootstrap.
    async fn create_tool_context(&self) -> ToolUseContext {
        let (live_mode, live_pre_plan, live_stripped) = match self.app_state.as_ref() {
            Some(state) => {
                let guard = state.read().await;
                (
                    guard.permission_mode.unwrap_or(self.config.permission_mode),
                    guard.pre_plan_mode,
                    guard.stripped_dangerous_rules.clone(),
                )
            }
            None => (self.config.permission_mode, None, None),
        };
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
            is_teammate: self.config.is_teammate,
            plan_mode_required: self.config.plan_mode_required,
            // Pre-resolve swarm identity once, so tools read from ctx
            // instead of process env. Falls back to env vars set by the
            // teammate spawner for cross-process scenarios. Env namespace
            // is `COCO_*` (coco-rs native) — see swarm_constants.
            agent_name: std::env::var("COCO_AGENT_NAME")
                .ok()
                .or_else(|| self.config.agent_id.clone()),
            team_name: std::env::var("COCO_TEAM_NAME").ok(),
            plan_verify_execution: self.config.plan_mode_settings.verify_execution,
            cancel: self.cancel.clone(),
            messages: Arc::new(RwLock::new(Vec::new())),
            permission_context: coco_types::ToolPermissionContext {
                mode: live_mode,
                additional_dirs: std::collections::HashMap::new(),
                allow_rules: std::collections::HashMap::new(),
                deny_rules: std::collections::HashMap::new(),
                ask_rules: std::collections::HashMap::new(),
                // Startup-derived capability (NOT a per-turn echo of the
                // live mode). Set once at bootstrap from the CLI
                // `--dangerously-skip-permissions` /
                // `--allow-dangerously-skip-permissions` flags plus
                // policy killswitch. Determines whether Plan-mode
                // auto-allow (evaluate.rs) and Shift+Tab cycle
                // (PermissionMode::next_in_cycle) can escalate into
                // `BypassPermissions`.
                bypass_available: self.config.bypass_permissions_available,
                pre_plan_mode: live_pre_plan,
                stripped_dangerous_rules: live_stripped,
                // Pre-resolved so the Plan-mode fallthrough can auto-allow
                // writes targeting the session plan file (TS parity:
                // `checkEditableInternalPath` + `isSessionPlanFile`).
                // For subagents this points at `{slug}-agent-{id}.md` so
                // the subagent's own plan file is auto-allowed.
                session_plan_file: self.config_home.as_ref().map(|ch| {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        self.config.project_dir.as_deref(),
                        self.config.plans_directory.as_deref(),
                    );
                    coco_context::get_plan_file_path(
                        &self.config.session_id,
                        &plans_dir,
                        self.config.agent_id.as_deref(),
                    )
                }),
            },
            tool_use_id: None,
            user_message_id: None,
            agent_id: self.config.agent_id.as_ref().map(coco_types::AgentId::new),
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
            mailbox: self
                .mailbox
                .clone()
                .unwrap_or_else(|| Arc::new(coco_tool::NoOpMailboxHandle)),
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
            plans_dir: self.config_home.as_ref().map(|ch| {
                coco_context::resolve_plans_directory(
                    ch,
                    self.config.project_dir.as_deref(),
                    self.config.plans_directory.as_deref(),
                )
            }),
            // Wrap the Arc in an `AppStateReadHandle` so tools can
            // only `.read()`. Writes go via `ToolResult::app_state_patch`
            // applied by the executor post-batch — a structural
            // guarantee that TS matches with queued context modifiers.
            app_state: self
                .app_state
                .as_ref()
                .map(|arc| coco_types::AppStateReadHandle::new(arc.clone())),
            local_denial_tracking: None,
            query_chain_id: None,
            query_depth: 0,
        }
    }
}

/// Per-call buffer used while consuming `StreamEvent`s for a single turn.
/// `input_json` is appended from `ToolCallDelta` chunks and parsed on
/// `ToolCallEnd`. Buffers are keyed by the provider-assigned `tool_call_id`.
struct StreamingToolCallBuffer {
    tool_name: String,
    input_json: String,
    complete: bool,
}

// Helpers moved to `crate::helpers`; engine only hosts the session-loop
// orchestration. Re-import from `helpers` at module top.

#[cfg(test)]
#[path = "engine.test.rs"]
mod tests;
