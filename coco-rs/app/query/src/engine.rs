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
use crate::tool_call_runner::ToolCallRunner;
use coco_config::EnvKey;
use coco_config::env;
use coco_context::FileHistoryState;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration;
use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_inference::StreamEvent;
use coco_messages::CostTracker;
use coco_messages::MessageHistory;
use coco_system_reminder::AttachmentType as ReminderAttachmentType;
use coco_system_reminder::SystemReminderOrchestrator;
use coco_system_reminder::TurnReminderInput;
use coco_system_reminder::count_human_turns;
use coco_system_reminder::inject_reminders;
use coco_system_reminder::run_turn_reminders;
use coco_tool::ToolRegistry;
use coco_tool::ToolUseContext;
use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::PermissionMode;
use coco_types::TokenUsage;
use coco_types::ToolAppState;

use crate::helpers::budget_pct_used;
use crate::helpers::convert_to_assistant_content;
use crate::helpers::drain_command_queue_into_history;
use crate::helpers::extract_last_assistant_text;
use crate::helpers::hook_outcome_to_status;
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
    /// Ordered fallback `ApiClient` chain. When non-empty,
    /// [`run_session_loop`](Self::run_session_loop) builds a
    /// per-session multi-slot [`ModelRuntime`] that walks slots in
    /// order on capacity-error streaks. Install via
    /// [`Self::with_fallback_client`] (one tier) or
    /// [`Self::with_fallback_clients`] (chain).
    fallback_clients: Vec<Arc<ApiClient>>,
    /// Optional half-open recovery policy. Empty = sticky
    /// fallback (post-switch the session stays on the fallback
    /// for the remainder). When set, the engine periodically
    /// probes the primary at turn entry and switches back on
    /// success. Install via [`Self::with_recovery_policy`].
    recovery_policy: Option<coco_config::FallbackRecoveryPolicy>,
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
    /// `NoOpMailboxHandle` in [`ToolContextFactory::build`]; swarm spawn paths
    /// install a real handle via [`Self::with_mailbox`].
    mailbox: Option<coco_tool::MailboxHandleRef>,
    /// Agent-runtime handle for `AgentTool` (subagent spawn / team
    /// management / background signalling). `None` resolves to
    /// `NoOpAgentHandle` in [`ToolContextFactory::build`]; the CLI /
    /// SDK / TUI runners install a real handle via
    /// [`Self::with_agent_handle`] so `AgentTool` calls reach the
    /// swarm runtime. TS parity: `runAgent.ts` is reachable from any
    /// model call; Rust sessions that skip installation intentionally
    /// restrict Agent tools to model-visible errors.
    agent_handle: Option<coco_tool::AgentHandleRef>,
    /// Session-scoped tool-input schema validator. One instance per
    /// engine so compiled validators cache across turns. Plan I3's
    /// Rust-side tightening — preparer runs this on both model
    /// input and any PreToolUse hook-rewritten input.
    tool_schema_validator: coco_tool::ToolSchemaValidator,
    /// Skill-runtime handle for `SkillTool`. Phase 7 routed skills
    /// off `AgentHandle::resolve_skill` onto this dedicated trait.
    /// `None` resolves to `NoOpSkillHandle` in the factory, which
    /// returns `SkillInvocationError::Unavailable` — the runner
    /// surfaces that as a clean model-visible error rather than
    /// panicking.
    skill_handle: Option<coco_tool::SkillHandleRef>,
    /// Persistent task-list store (V2, `TaskCreate`/`TaskUpdate`/etc.).
    /// `None` resolves to `NoOpTaskListHandle` — the V2 tools then
    /// return errors on write, matching TS's "no store configured"
    /// behavior. Install via [`Self::with_task_list`].
    task_list: Option<coco_tool::TaskListHandleRef>,
    /// Per-agent ephemeral todo store (V1, `TodoWrite`). Defaults to
    /// an in-memory instance when absent.
    todo_list: Option<coco_tool::TodoListHandleRef>,
    /// Bundle of per-subsystem reminder sources. Populated by CLI /
    /// SDK callers via [`Self::with_reminder_sources`]. Empty default
    /// ⇒ cross-crate reminders silently skip (matches TS behavior
    /// when the corresponding manager isn't initialized).
    reminder_sources: coco_system_reminder::ReminderSources,
    /// Channel for silent attachment events produced by owner crates
    /// (hooks, permissions, commands, core/tool, skills). Drained at the
    /// head of each outer-loop iteration so the `Message::Attachment`
    /// entries land in history before prompt build.
    ///
    /// Sender cloned to [`Self::attachment_emitter`] for plumbing into
    /// owner crates; receiver is drained by `drain_attachment_inbox`.
    attachment_tx: tokio::sync::mpsc::UnboundedSender<coco_types::AttachmentMessage>,
    attachment_rx: Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<coco_types::AttachmentMessage>>,
    >,
}

impl QueryEngine {
    pub fn new(
        config: QueryEngineConfig,
        client: Arc<ApiClient>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        hooks: Option<Arc<HookRegistry>>,
    ) -> Self {
        let (attachment_tx, attachment_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            config,
            client,
            fallback_clients: Vec::new(),
            recovery_policy: None,
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
            agent_handle: None,
            skill_handle: None,
            tool_schema_validator: coco_tool::ToolSchemaValidator::new(),
            task_list: None,
            todo_list: None,
            reminder_sources: coco_system_reminder::ReminderSources::default(),
            attachment_tx,
            attachment_rx: Arc::new(tokio::sync::Mutex::new(attachment_rx)),
        }
    }

    /// A clone-friendly emitter handle for owning crates (hooks /
    /// permissions / commands / core/tool / skills) so they can push
    /// `Message::Attachment` entries into this session's history without
    /// direct access to the engine. Drained once per outer-loop turn.
    pub fn attachment_emitter(&self) -> coco_types::AttachmentEmitter {
        coco_types::AttachmentEmitter::new(self.attachment_tx.clone())
    }

    /// Drain any silent attachments emitted since the last turn into
    /// `history`. Called at the head of each outer-loop iteration.
    /// Returns the number of drained attachments for telemetry.
    async fn drain_attachment_inbox(&self, history: &mut coco_messages::MessageHistory) -> usize {
        let mut count = 0;
        let mut rx = self.attachment_rx.lock().await;
        while let Ok(att) = rx.try_recv() {
            history.messages.push(coco_types::Message::Attachment(att));
            count += 1;
        }
        count
    }

    /// Install the per-subsystem reminder source bundle. Each
    /// `Some(Arc<dyn XxxSource>)` field powers a category of
    /// system-reminders that needs state from an owning crate
    /// (hooks, LSP, tasks, skills, MCP, swarm, bridge, memory).
    /// Omitted sources → corresponding reminders silently skip.
    ///
    /// TS parity: this is the analog of `toolUseContext.options.*`
    /// that TS's `getAttachments` reads from.
    pub fn with_reminder_sources(mut self, sources: coco_system_reminder::ReminderSources) -> Self {
        self.reminder_sources = sources;
        self
    }

    /// Install a mailbox handle for swarm teammate messaging.
    pub fn with_mailbox(mut self, mailbox: coco_tool::MailboxHandleRef) -> Self {
        self.mailbox = Some(mailbox);
        self
    }

    /// Install the real [`AgentHandle`](coco_tool::AgentHandle) so
    /// `AgentTool` invocations route to the swarm / subagent
    /// runtime. Without this the factory defaults to
    /// `NoOpAgentHandle` and every `AgentTool` call returns a clean
    /// "not available in this context" error — fine for tests, but
    /// CLI / SDK / TUI runners should install a real handle at
    /// bootstrap.
    pub fn with_agent_handle(mut self, handle: coco_tool::AgentHandleRef) -> Self {
        self.agent_handle = Some(handle);
        self
    }

    /// Install a single fallback [`ApiClient`]. Convenience wrapper
    /// for the common one-tier case; equivalent to
    /// `.with_fallback_clients(vec![client])`.
    pub fn with_fallback_client(mut self, client: Arc<ApiClient>) -> Self {
        self.fallback_clients = vec![client];
        self
    }

    /// Install an ordered chain of fallback [`ApiClient`]s. The
    /// engine walks slot 0 → slot 1 → … on capacity-error streaks
    /// via [`ModelRuntime::advance`]. Empty input = no fallback.
    pub fn with_fallback_clients(mut self, clients: Vec<Arc<ApiClient>>) -> Self {
        self.fallback_clients = clients;
        self
    }

    /// Install a half-open recovery policy for the session. Enables
    /// periodic probes back to primary after a fallback switch;
    /// see [`coco_config::FallbackRecoveryPolicy`]. Omitting this
    /// call keeps the default sticky-fallback behavior.
    pub fn with_recovery_policy(mut self, policy: coco_config::FallbackRecoveryPolicy) -> Self {
        self.recovery_policy = Some(policy);
        self
    }

    /// Install the real [`SkillHandle`](coco_tool::SkillHandle) so
    /// `SkillTool` invocations route to the skill runtime (inline
    /// expansion or forked subagent). Without this the factory
    /// defaults to `NoOpSkillHandle` and every skill call returns
    /// `SkillInvocationError::Unavailable` — the runner surfaces
    /// that as a model-visible error.
    pub fn with_skill_handle(mut self, handle: coco_tool::SkillHandleRef) -> Self {
        self.skill_handle = Some(handle);
        self
    }

    /// Install the durable task-list store (V2 task tools).
    pub fn with_task_list(mut self, handle: coco_tool::TaskListHandleRef) -> Self {
        self.task_list = Some(handle);
        self
    }

    /// Install the ephemeral per-agent todo store (V1 TodoWrite).
    pub fn with_todo_list(mut self, handle: coco_tool::TodoListHandleRef) -> Self {
        self.todo_list = Some(handle);
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
    /// first batch's [`ToolContextFactory::build`] sees a concrete mode. If
    /// already `Some(_)` (e.g. session resumed, prior-run state
    /// carried), the existing value is preserved — user + tool
    /// intent trumps config. TS parity: `appState` is
    /// initialized-once at session-create and never re-seeded from
    /// config afterward.
    pub fn with_app_state(mut self, app_state: Arc<RwLock<ToolAppState>>) -> Self {
        // Bootstrap the live mode on first attach. This is a one-shot
        // write — subsequent runs that reuse the same app_state see
        // the preserved value rather than an overwrite.
        if let Ok(mut guard) = app_state.try_write()
            && guard.permission_mode.is_none()
        {
            guard.permission_mode = Some(self.config.permission_mode);
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

        // Build the per-session ModelRuntime. When the caller
        // installed fallback clients via `with_fallback_client(s)`,
        // the runtime holds a multi-slot chain and walks it on
        // capacity-error streaks via `advance()`.
        //
        // Fallback trigger (TS parity, `services/api/withRetry.ts:335`):
        // after `MAX_529_RETRIES` consecutive `Overloaded` (529/503)
        // responses from the active slot, the next turn advances to
        // the next slot. The engine tracks consecutive capacity
        // errors because provider-layer retries are internal to the
        // vercel-ai crates — this counter only ticks when the retry
        // layer gives up and surfaces an error to us.
        let mut model_runtime = crate::model_runtime::ModelRuntime::new(
            self.client.clone(),
            self.fallback_clients.clone(),
        );
        if let Some(policy) = self.recovery_policy {
            model_runtime = model_runtime.with_recovery_policy(policy);
        }
        /// TS: `MAX_529_RETRIES = 3` in `services/api/withRetry.ts:54`.
        const MAX_CONSECUTIVE_CAPACITY_ERRORS: u32 = 3;
        let mut consecutive_capacity_errors: u32 = 0;
        // TS `input`-parameter parity: tracks the UUID of the last user
        // message that has already been handed to the UserPrompt-tier
        // reminders. Prevents duplicate `at_mentioned_files` /
        // `agent_mentions` / `ultrathink_effort` emissions across
        // tool-result iterations of the same human turn.
        let mut reminder_last_user_input_uuid: Option<uuid::Uuid> = None;
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

        // ── Progress-event forwarder ──
        //
        // Spawn one drain task per session. Tools send `ToolProgress`
        // updates through `ctx.progress_tx`; the drain fans them out
        // to:
        //
        //   1. `TuiOnlyEvent::ToolProgress { tool_use_id, data }` —
        //      every event, unthrottled, carries the raw payload for
        //      the TUI to render progress bars or byte counts.
        //
        //   2. `ServerNotification::ToolProgress(ToolProgressParams)` —
        //      TS-parity wire event. Only emitted for
        //      `bash_progress` / `powershell_progress` payload types
        //      and throttled to ≤1 per 30 s per
        //      `parent_tool_use_id` (or `tool_use_id` if the parent
        //      is absent), matching `utils/queryHelpers.ts:99-189`.
        //
        // TS parity: `onProgress` in `StreamingToolExecutor` loops
        // progress yielded from the tool generator back to the
        // streaming UI; `normalizeMessage` throttles the SDK-facing
        // version separately. Rust collapses both into one drain
        // task because there's no separate normalization stage.
        //
        // Lifecycle: the tx is cloned into every `ToolUseContext`
        // built for this session. When the session loop exits, the
        // last tx clone (owned here) drops, the rx closes, and the
        // drain task finishes naturally — no explicit await needed.
        let (progress_tx_session, mut progress_rx_session) =
            tokio::sync::mpsc::unbounded_channel::<coco_tool::ToolProgress>();
        let progress_event_tx = event_tx.clone();
        let _progress_drain = tokio::spawn(async move {
            let mut throttle = ProgressThrottle::new();
            while let Some(progress) = progress_rx_session.recv().await {
                drain_one_progress(&progress_event_tx, progress, &mut throttle).await;
            }
        });

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
        // Plan/workflow / phase-4 / agent-count values are fed into the
        // orchestrator's `TurnReminderInput` below. `PlanModeReminder` is
        // now the per-turn side-effect driver (mode reconcile + mailbox
        // polling + leader-pending-approvals) and no longer owns
        // workflow state.
        let plans_dir = crate::plan_mode_reminder::PlanModeReminder::resolve_plans_dir(
            self.config_home.as_deref(),
            self.config.project_dir.as_deref(),
            self.config.plans_directory.as_deref(),
        );
        let mut plan_reminder = crate::plan_mode_reminder::PlanModeReminder::new(
            self.config.permission_mode,
            Some(self.config.session_id.clone()),
            self.config.agent_id.clone(),
            plans_dir,
            self.app_state.clone(),
        );
        // Wire mailbox for swarm polling if identity is set and a mailbox
        // handle is installed. Agent + team names come from env vars
        // (set by the swarm spawner); mirror `swarm_identity::get_agent_name`
        // env fallback. We keep the env read here rather than threading
        // via ctx because the reminder is engine-level (no ToolUseContext).
        // Env namespace is `COCO_*` — see swarm_constants.
        let agent_name_env = env::env_opt(EnvKey::CocoAgentName);
        let team_name_env = env::env_opt(EnvKey::CocoTeamName);
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
        // Install the protocol-event sink so leader-pending-approval
        // polling can surface `PlanApprovalRequested` to the TUI in
        // addition to injecting the LLM-prompt attachment. Absent sink
        // (SDK-only / headless) means the overlay simply never fires.
        if let Some(tx) = event_tx.clone() {
            plan_reminder = plan_reminder.with_event_sink(tx);
        }

        // System-reminder orchestrator — owns reminder emission for the
        // whole session. The orchestrator is Send+Sync and accumulates
        // per-attachment throttle state across turns.
        //
        // `plan_reminder` above is retained for non-reminder side effects
        // (mode reconciliation, teammate mailbox polling, leader-pending-
        // approvals), called per turn via `turn_start_side_effects_only`.
        // The reminder emission itself (plan/auto/todo/task/critical/
        // compaction/date-change) moves here.
        // Settings-driven reminder config (TS `settings.json` →
        // `coco_config::Settings.system_reminder`). Cloned because the
        // orchestrator owns its own copy for the session — subsequent
        // settings reloads won't retroactively disable reminders until
        // the next engine build.
        let reminder_config = self.config.system_reminder.clone();
        let reminder_orchestrator =
            SystemReminderOrchestrator::new(reminder_config).with_default_generators();
        // Todo-list lookup key: TS `agentId ?? sessionId`.
        let reminder_todo_key = self
            .config
            .agent_id
            .clone()
            .unwrap_or_else(|| self.config.session_id.clone());
        // Model context window — exposed to the compaction reminder
        // generator. Effective = 90% of window (reserve 10% for output),
        // matching the same approximation `coco-compact` uses.
        let reminder_context_window = self.config.context_window;
        let reminder_effective_window = (reminder_context_window * 9) / 10;

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

            // Turn-start reminder pipeline (Phase D.3):
            //
            // 1. Run non-reminder side effects (mode reconciliation +
            //    mailbox polling + leader pending-approvals) — these
            //    MUTATE app_state (setting `needs_plan_mode_exit_attachment`
            //    / `has_exited_plan_mode` when detecting unannounced mode
            //    transitions). Must run BEFORE the orchestrator reads
            //    app_state below.
            plan_reminder
                .turn_start_side_effects_only(&mut history)
                .await;

            // 2. Build orchestrator input from engine state + current
            //    app_state snapshot.
            //
            // `turn_number` uses **human turns** (non-meta user messages)
            // so plan-mode / auto-mode throttle cadence matches TS
            // (counts human turns, not LLM iterations). Tool-result
            // rounds within one human turn share the same counter value
            // so reminders don't spam mid-turn.
            let reminder_tools: Vec<String> = self
                .tools
                .loaded_tools()
                .iter()
                .map(|t| t.name().to_string())
                .collect();
            let pm_settings = &self.config.plan_mode_settings;
            let workflow_rm = match pm_settings.workflow {
                coco_config::PlanModeWorkflow::FivePhase => coco_context::PlanWorkflow::FivePhase,
                coco_config::PlanModeWorkflow::Interview => coco_context::PlanWorkflow::Interview,
            };
            let phase4_rm = match pm_settings.phase4_variant {
                coco_config::PlanPhase4Variant::Standard => coco_context::Phase4Variant::Standard,
                coco_config::PlanPhase4Variant::Trim => coco_context::Phase4Variant::Trim,
                coco_config::PlanPhase4Variant::Cut => coco_context::Phase4Variant::Cut,
                coco_config::PlanPhase4Variant::Cap => coco_context::Phase4Variant::Cap,
            };
            // Plan file path / existence — same resolver the deprecated
            // emission path uses, so both paths agree on the filesystem state.
            let (reminder_plan_path, reminder_plan_exists) =
                match (self.config_home.as_deref(), &self.config.session_id) {
                    (Some(ch), sid) if !sid.is_empty() => {
                        let plans_dir = coco_context::resolve_plans_directory(
                            ch,
                            self.config.project_dir.as_deref(),
                            self.config.plans_directory.as_deref(),
                        );
                        let path = coco_context::get_plan_file_path(
                            sid,
                            &plans_dir,
                            self.config.agent_id.as_deref(),
                        );
                        let exists = path.exists();
                        (Some(path), exists)
                    }
                    _ => (None, false),
                };

            let reminder_human_turn_number = count_human_turns(&history.messages);

            // Take an app_state snapshot so the input struct holds an
            // immutable borrow; any post-emit clearing happens after the
            // orchestrator returns.
            let app_state_snapshot = match self.app_state.as_ref() {
                Some(state) => state.read().await.clone(),
                None => ToolAppState::default(),
            };

            // Seed the orchestrator's throttle state from `app_state` so
            // reminder cadence survives across `run_session_loop`
            // invocations. Each `run_plan_mode_turn` / `run_internal`
            // call constructs a fresh orchestrator but `app_state`
            // persists — without seeding, turn 2 of a multi-turn test
            // would see an empty throttle and fire a second reminder.
            //
            // Implied `last_generated_turn`: the current human-turn
            // counter minus the stored gap. Tool-result rounds within
            // the same human turn keep the same value, so the throttle
            // correctly blocks within-turn re-firing.
            if app_state_snapshot.plan_mode_attachment_count > 0 {
                let gap = i32::try_from(app_state_snapshot.plan_mode_turns_since_last_attachment)
                    .unwrap_or(i32::MAX);
                let last_gen_turn = reminder_human_turn_number.saturating_sub(gap);
                reminder_orchestrator.throttle().seed_state(
                    ReminderAttachmentType::PlanMode,
                    coco_system_reminder::ThrottleState {
                        last_generated_turn: Some(last_gen_turn),
                        session_count: i32::try_from(app_state_snapshot.plan_mode_attachment_count)
                            .unwrap_or(i32::MAX),
                        trigger_turn: None,
                    },
                );
            }

            // TS `autoModeStateModule?.isAutoModeActive()`. `None` means the
            // engine was built without a permissions auto-mode state — auto
            // mode is therefore inactive, matching TS's `?? false` fallback.
            let reminder_auto_classifier_active = self
                .auto_mode_state
                .as_ref()
                .map(|s| s.is_active())
                .unwrap_or(false);
            let reminder_permission_mode = app_state_snapshot
                .permission_mode
                .unwrap_or(self.config.permission_mode);
            let reminder_is_plan_mode = reminder_permission_mode == PermissionMode::Plan;
            let reminder_is_auto_mode = reminder_permission_mode == PermissionMode::Auto
                || (reminder_permission_mode == PermissionMode::Plan
                    && reminder_auto_classifier_active);
            // TS `isTodoV2Enabled()` — coco-rs derives this from whether the
            // V2 task mutation tools are actually loaded into the session.
            // `TASK_MANAGEMENT_TOOLS` is the `[TaskCreate, TaskUpdate]` set
            // (matches TS `getTaskReminderTurnCounts`); V2 is active when
            // either mutation tool is wired into the current registry —
            // read-only task tools alone aren't enough.
            let reminder_task_v2_enabled =
                coco_system_reminder::TASK_MANAGEMENT_TOOLS.iter().any(|t| {
                    let wire = t.as_str();
                    reminder_tools.iter().any(|name| name == wire)
                });
            // TS `isAutoCompactEnabled()` — a user-facing toggle. coco-rs
            // surfaces it on `QueryEngineConfig::auto_compact_enabled` so
            // the SDK / CLI / TUI can control it per session without
            // re-reading settings from disk.
            let reminder_auto_compact_enabled = self.config.auto_compact_enabled;
            // TS `getDeferredToolsDelta` — diff current tools against the
            // last announced set stored on app_state. Non-empty added or
            // removed triggers the `deferred_tools_delta` reminder.
            let reminder_deferred_tools_delta =
                compute_tools_delta(&reminder_tools, &app_state_snapshot.last_announced_tools);
            // Clone the tool list for post-emit bookkeeping (the main
            // `reminder_tools` is moved into `TurnReminderInput::tools`).
            let reminder_tools_clone = reminder_tools.clone();
            // TS `getAgentListingDeltaAttachment` — diff the current
            // agent-type set (from `SessionBootstrap`) against the
            // last-announced set on app_state.
            let reminder_current_agents: Vec<String> = self
                .session_bootstrap
                .as_ref()
                .map(|b| b.agents.clone())
                .unwrap_or_default();
            let reminder_agent_listing_delta = compute_agents_delta(
                &reminder_current_agents,
                &app_state_snapshot.last_announced_agents,
            );
            // TS date-change latch: current local ISO date vs. the one
            // stored on `ToolAppState.last_emitted_date`. When they
            // differ, emit once + update the latch. Runs at turn start
            // so the reminder sees today's date even for long-running
            // sessions that cross midnight.
            let reminder_new_date = self.observe_date_change().await;

            // TS `getAttachments(input, ...)` — the user's raw prompt
            // text for this turn. Extract from the most-recent non-meta
            // user message's text content; used by both the
            // ultrathink-keyword gate and mention-based reminders.
            //
            // TS parity: `input` is non-null only on the first tool-loop
            // iteration of a human turn, not on subsequent tool-result
            // rounds (query.ts nulls it out). coco-rs tracks the last
            // user-message UUID that has already been reminder-scanned
            // and skips re-parsing it so the user-input tier fires once
            // per human turn, not once per tool-result iteration.
            let reminder_current_user_uuid = history.messages.iter().rev().find_map(|m| match m {
                Message::User(u) => Some(u.uuid),
                _ => None,
            });
            let reminder_is_new_human_turn =
                reminder_current_user_uuid != reminder_last_user_input_uuid;
            let reminder_user_input: Option<String> = if reminder_is_new_human_turn {
                reminder_last_user_input_uuid = reminder_current_user_uuid;
                latest_user_input_text(&history)
            } else {
                None
            };
            let reminder_mentions: Vec<coco_context::user_input::Mention> = reminder_user_input
                .as_deref()
                .map(|raw| coco_context::user_input::process_user_input(raw).mentions)
                .unwrap_or_default();
            let reminder_at_mentioned_files: Vec<coco_system_reminder::MentionedFileEntry> =
                reminder_mentions
                    .iter()
                    .filter(|m| {
                        matches!(
                            m.mention_type,
                            coco_context::user_input::MentionType::FilePath
                        )
                    })
                    .map(|m| coco_system_reminder::MentionedFileEntry {
                        filename: m.text.clone(),
                        display_path: m.text.clone(),
                    })
                    .collect();
            let reminder_agent_mentions: Vec<coco_system_reminder::AgentMentionEntry> =
                reminder_mentions
                    .iter()
                    .filter(|m| {
                        matches!(m.mention_type, coco_context::user_input::MentionType::Agent)
                    })
                    .map(|m| coco_system_reminder::AgentMentionEntry {
                        agent_type: m.text.clone(),
                    })
                    .collect();

            // TS `toolUseContext.options.*` bag analog — fan-out to every
            // per-subsystem source (hooks / LSP / tasks / skills / MCP /
            // swarm / IDE / memory) in parallel, with per-source timeout
            // + error-to-default. Empty `ReminderSources` → all defaults.
            let reminder_mentioned_paths: Vec<std::path::PathBuf> = reminder_mentions
                .iter()
                .filter(|m| {
                    matches!(
                        m.mention_type,
                        coco_context::user_input::MentionType::FilePath
                    )
                })
                .map(|m| std::path::PathBuf::from(&m.text))
                .collect();

            let reminder_source_timeout = std::time::Duration::from_millis(
                if reminder_orchestrator.config().timeout_ms > 0 {
                    reminder_orchestrator.config().timeout_ms as u64
                } else {
                    coco_system_reminder::DEFAULT_TIMEOUT_MS as u64
                },
            );
            let materialized = self
                .reminder_sources
                .materialize(coco_system_reminder::MaterializeContext {
                    config: reminder_orchestrator.config(),
                    agent_id: self.config.agent_id.as_deref(),
                    user_input: reminder_user_input.as_deref(),
                    mentioned_paths: &reminder_mentioned_paths,
                    // `just_compacted` is wired in P3 when services/compact
                    // exposes the per-turn boundary signal.
                    just_compacted: false,
                    per_source_timeout: reminder_source_timeout,
                })
                .await;

            // Part 1 silent reminder: intersect every path this turn
            // might try to load (@-mentions + nested memory + relevant
            // memory prefetch) with the session file-read cache. Paths
            // whose mtime still matches disk are "already loaded into
            // context" — we emit a silent dedup marker so downstream
            // tooling (transcript, telemetry) knows the model has current
            // content for those paths. Mirrors TS `already_read_file`
            // emission surface area (`utils/attachments.ts:3100`).
            let reminder_already_read_file_paths: Vec<std::path::PathBuf> =
                if let Some(frs) = &self.file_read_state {
                    let mut candidates: Vec<std::path::PathBuf> = reminder_mentioned_paths.clone();
                    candidates.extend(
                        materialized
                            .nested_memories
                            .iter()
                            .map(|m| std::path::PathBuf::from(&m.path)),
                    );
                    candidates.extend(
                        materialized
                            .relevant_memories
                            .iter()
                            .map(|m| std::path::PathBuf::from(&m.path)),
                    );
                    if candidates.is_empty() {
                        Vec::new()
                    } else {
                        // Dedup while preserving first-seen order so the
                        // resulting list is deterministic across turns.
                        let mut seen = std::collections::HashSet::new();
                        candidates.retain(|p| seen.insert(p.clone()));
                        let guard = frs.read().await;
                        guard.unchanged_paths(&candidates).await
                    }
                } else {
                    Vec::new()
                };

            let reminder_input = TurnReminderInput {
                config: reminder_orchestrator.config(),
                turn_number: reminder_human_turn_number,
                agent_id: self.config.agent_id.clone(),
                user_input: reminder_user_input.clone(),
                last_human_turn_uuid: history.messages.iter().rev().find_map(|m| match m {
                    Message::User(u) => Some(u.uuid),
                    _ => None,
                }),
                plan_file_path: reminder_plan_path,
                plan_exists: reminder_plan_exists,
                plan_workflow: workflow_rm,
                phase4_variant: phase4_rm,
                explore_agent_count: pm_settings.explore_agent_count,
                plan_agent_count: pm_settings.plan_agent_count,
                is_plan_interview_phase: false,
                app_state: &app_state_snapshot,
                fallback_permission_mode: self.config.permission_mode,
                is_auto_classifier_active: reminder_auto_classifier_active,
                tools: reminder_tools,
                is_task_v2_enabled: reminder_task_v2_enabled,
                history: &history,
                todo_key: reminder_todo_key.clone(),
                is_auto_compact_enabled: reminder_auto_compact_enabled,
                context_window: reminder_context_window,
                effective_context_window: reminder_effective_window,
                used_tokens: total_usage.input_tokens,
                new_date: reminder_new_date,
                has_pending_plan_verification: app_state_snapshot.pending_plan_verification,
                // Phase 1 engine-local inputs.
                total_cost_usd: cost_tracker.total_cost_usd(),
                max_budget_usd: self.config.max_budget_usd,
                // Injected at turn start — TS `getTurnOutputTokens()` is zero
                // at this point; cumulative session count comes from usage.
                output_tokens_turn: 0,
                output_tokens_session: total_usage.output_tokens,
                // Not yet wired (requires feature('TOKEN_BUDGET')-equivalent).
                output_token_budget: None,
                // Companion subsystem lives in a future Buddy crate; for now
                // suppress the reminder by leaving these unset.
                companion_name: None,
                companion_species: None,
                has_prior_companion_intro: false,
                deferred_tools_delta: reminder_deferred_tools_delta.clone(),
                agent_listing_delta: reminder_agent_listing_delta.clone(),
                // McpSource.instructions() returns the current per-server
                // map; engine diffs against `last_announced_mcp_instructions`
                // to produce the delta (same pattern as deferred_tools_delta).
                mcp_instructions_delta: compute_mcp_instructions_delta(
                    &materialized.mcp_instructions_current,
                    &app_state_snapshot.last_announced_mcp_instructions,
                ),
                // Phase 3: cross-crate state flows via `ReminderSources`.
                // Sources that aren't wired → default output → generator skips.
                hook_events: materialized.hook_events,
                diagnostics: materialized.diagnostics,
                // TS `getOutputStyleAttachment` — reads style name from
                // `SessionBootstrap` (CLI-resolved from `settings.output_style`).
                // This is a simple read, not cross-crate state, so no Source
                // trait is needed.
                output_style: self
                    .session_bootstrap
                    .as_ref()
                    .and_then(|b| b.output_style.as_ref())
                    .filter(|s| !s.is_empty())
                    .map(|name| coco_system_reminder::OutputStyleSnapshot { name: name.clone() }),
                queued_commands: self
                    .command_queue
                    .snapshot_for_reminder(self.config.agent_id.as_deref())
                    .await,
                task_statuses: materialized.task_statuses,
                // SkillsSource wins when present; else fall back to
                // SessionBootstrap names-only listing.
                skill_listing: materialized.skill_listing.or_else(|| {
                    self.session_bootstrap
                        .as_ref()
                        .filter(|b| !b.skills.is_empty())
                        .map(|b| {
                            b.skills
                                .iter()
                                .map(|s| format!("- {s}"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                }),
                invoked_skills: materialized.invoked_skills,
                teammate_mailbox: materialized.teammate_mailbox,
                team_context: materialized.team_context,
                agent_pending_messages: materialized.agent_pending_messages,
                // Phase 4: mention-based reminders are populated from
                // `process_user_input`. MCP resources come from the MCP
                // source; IDE state is a main-thread reminder source.
                at_mentioned_files: reminder_at_mentioned_files,
                mcp_resources: materialized.mcp_resources,
                agent_mentions: reminder_agent_mentions,
                ide_selection: materialized.ide_selection,
                ide_opened_file: materialized.ide_opened_file,
                // Memory reminders from MemorySource.
                nested_memories: materialized.nested_memories,
                relevant_memories: materialized.relevant_memories,
                // Silent reminder-native attachments (Part 1).
                // `already_read_file_paths`: intersection of this turn's
                // @-mentioned paths with the `FileReadState` cache where
                // mtime still matches disk — computed above via
                // `FileReadState::unchanged_paths`.
                // `edited_image_file_paths`: reserved for a future image-
                // mtime tracker. Text `FileReadState` is text-only; image
                // drift detection would need a parallel cache.
                already_read_file_paths: reminder_already_read_file_paths,
                edited_image_file_paths: Vec::new(),
            };
            let reminders = run_turn_reminders(&reminder_orchestrator, reminder_input).await;

            // 3. Post-emit bookkeeping on app_state. Writing AFTER the
            //    orchestrator read ensures we don't clear a flag whose
            //    reminder got throttled (so it can fire next turn).
            //
            //    Covers three concerns:
            //    - One-shot flags consumed by the generators that fired
            //      (PlanModeExit / AutoModeExit / PlanModeReentry).
            //    - Cadence counters the TUI / tests observe via app_state
            //      (`plan_mode_attachment_count` +
            //      `plan_mode_turns_since_last_attachment`). These mirror
            //      the ThrottleManager state but are exposed on app_state
            //      for TS parity with `getAppState().planModeAttachmentCount`.
            let stale_plan_exit_flag =
                app_state_snapshot.needs_plan_mode_exit_attachment && reminder_is_plan_mode;
            let stale_auto_exit_flag =
                app_state_snapshot.needs_auto_mode_exit_attachment && reminder_is_auto_mode;
            let needs_reminder_bookkeeping =
                !reminders.is_empty() || stale_plan_exit_flag || stale_auto_exit_flag;
            if needs_reminder_bookkeeping && self.app_state.is_some() {
                let fired_types: std::collections::HashSet<ReminderAttachmentType> =
                    reminders.iter().map(|r| r.attachment_type).collect();
                if let Some(state) = self.app_state.as_ref() {
                    let mut guard = state.write().await;
                    // TS clears stale one-shot exit flags when the engine is
                    // still in the matching mode instead of preserving them
                    // for a later, unrelated turn.
                    if stale_plan_exit_flag {
                        guard.needs_plan_mode_exit_attachment = false;
                    }
                    if stale_auto_exit_flag {
                        guard.needs_auto_mode_exit_attachment = false;
                    }
                    if fired_types.contains(&ReminderAttachmentType::PlanModeExit) {
                        guard.needs_plan_mode_exit_attachment = false;
                        // TS: exit resets the plan-mode cadence cycle.
                        guard.plan_mode_attachment_count = 0;
                        guard.plan_mode_turns_since_last_attachment = 0;
                        guard.last_human_turn_uuid_seen = None;
                    }
                    if fired_types.contains(&ReminderAttachmentType::AutoModeExit) {
                        guard.needs_auto_mode_exit_attachment = false;
                    }
                    if fired_types.contains(&ReminderAttachmentType::PlanModeReentry) {
                        guard.has_exited_plan_mode = false;
                    }
                    if fired_types.contains(&ReminderAttachmentType::PlanMode) {
                        // Bump the TS-parity cadence counter + reset the
                        // "turns since last attachment" counter so the TUI
                        // and integration tests observe the same cadence
                        // state as the pre-Phase-D PlanModeReminder flow.
                        guard.plan_mode_attachment_count =
                            guard.plan_mode_attachment_count.saturating_add(1);
                        guard.plan_mode_turns_since_last_attachment = 0;
                        // Stamp the current human-turn UUID so subsequent
                        // tool-result rounds sharing the same UUID don't
                        // advance the counter (mirror of the old
                        // `observe_turn_and_count` behavior).
                        if let Some(uuid) = history.messages.iter().rev().find_map(|m| match m {
                            Message::User(u) => Some(u.uuid),
                            _ => None,
                        }) {
                            guard.last_human_turn_uuid_seen = Some(uuid);
                        }
                    }
                    // TS `getDeferredToolsDelta` replaces the announced
                    // set with the current tool list after successful
                    // emission. Subsequent turns then diff against the
                    // fresh baseline.
                    if fired_types.contains(&ReminderAttachmentType::DeferredToolsDelta) {
                        guard.last_announced_tools = reminder_tools_clone.iter().cloned().collect();
                    }
                    // Same pattern for the agent-listing delta.
                    if fired_types.contains(&ReminderAttachmentType::AgentListingDelta) {
                        guard.last_announced_agents =
                            reminder_current_agents.iter().cloned().collect();
                    }
                    // Same pattern for the MCP-instructions delta.
                    if fired_types.contains(&ReminderAttachmentType::McpInstructionsDelta) {
                        guard.last_announced_mcp_instructions =
                            materialized.mcp_instructions_current.clone();
                    }
                }
            }

            // 4. Inject reminder messages into history. Model-visible
            //    reminders append to `history`; silent reminders
            //    (`Coverage::SilentReminder` + `ReminderOutput::Silent*`)
            //    come back as `display_only` so they never leak into the
            //    API call but stay observable for UI / telemetry.
            // Drain any silent attachments queued by owner crates
            // (hooks / permissions / tools / etc.) since the prior turn.
            // Must happen BEFORE inject_reminders so the reminder pipeline
            // sees any cross-crate-produced attachments in history.
            let drained = self.drain_attachment_inbox(&mut history).await;
            if drained > 0 {
                tracing::debug!(
                    target: "coco::attachment_inbox",
                    drained,
                    "drained silent attachments into history"
                );
            }

            let display_only = inject_reminders(reminders, &mut history.messages);
            for msg in &display_only {
                tracing::debug!(
                    target: "coco::system_reminder::display_only",
                    injected = ?msg,
                    "silent reminder routed to display-only sink"
                );
            }

            // Build prompt from history
            let prompt = self.build_prompt(&history);
            let tool_defs = self.build_tool_definitions(&app_state_snapshot).await;

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

            // ── Phase 9: Streaming tool scheduling ──
            //
            // When `config.streaming_tool_execution = true`, safe
            // tools start executing the moment their input buffer
            // completes, rather than waiting for the whole stream
            // to finish. The `StreamingHandle` owns the inflight
            // JoinSet and the gate that preserves TS parity
            // (`canExecuteTool`: no safe-during-unsafe mid-stream).
            //
            // We build the shared ctx (Arc'd so spawned tasks can
            // hold owned clones) + `StreamingHandle` here, ahead of
            // the stream loop. When streaming is off, the whole
            // block is an unused `None` and the legacy batch path
            // below handles execution post-Finish.
            let streaming_enabled = self.config.streaming_tool_execution;
            let streaming_ctx: Option<Arc<ToolUseContext>> = if streaming_enabled {
                let base = self
                    .tool_context_factory(hook_tx_opt.as_ref())
                    .build(crate::tool_context::ToolContextOverrides {
                        user_message_id: Some(user_msg_uuid.clone()),
                        progress_tx: Some(progress_tx_session.clone()),
                        current_model_name: Some(model_runtime.current_model_name().to_string()),
                    })
                    .await;
                Some(Arc::new(base))
            } else {
                None
            };
            let mut streaming_handle = streaming_ctx.as_ref().map(|ctx_arc| {
                let executor_base = coco_tool::StreamingToolExecutor::new();
                let executor = Arc::new(match self.app_state.as_ref() {
                    Some(state) => executor_base.with_app_state(state.clone()),
                    None => executor_base,
                });
                let ctx_for_closure = ctx_arc.clone();
                let hooks_for_closure = self.hooks.clone();
                let orchestration_for_closure = self.orchestration_ctx();
                let hook_tx_for_closure = hook_tx_opt.clone();
                executor.streaming_handle(move |prepared, _runtime| {
                    let ctx = ctx_for_closure.clone();
                    let hooks = hooks_for_closure.clone();
                    let orchestration_ctx = orchestration_for_closure.clone();
                    let hook_tx = hook_tx_for_closure.clone();
                    Box::pin(async move {
                        let effective_input = prepared.parsed_input.clone();
                        let execute_result = tokio::select! {
                            r = prepared.tool.execute(effective_input.clone(), &ctx) => r,
                            () = ctx.cancel.cancelled() => Err(coco_tool::ToolError::Cancelled),
                        };
                        crate::tool_outcome_builder::build_outcome_from_execution(
                            crate::tool_outcome_builder::RunOneTail {
                                tool_use_id: prepared.tool_use_id.clone(),
                                tool_id: prepared.tool_id.clone(),
                                tool_name: prepared.tool.name().to_string(),
                                model_index: prepared.model_index,
                                tool: prepared.tool,
                                effective_input,
                                execute_result,
                                hooks: hooks.as_ref(),
                                orchestration_ctx,
                                hook_tx: hook_tx.as_ref(),
                            },
                        )
                        .await
                    })
                        as std::pin::Pin<
                            Box<
                                dyn std::future::Future<
                                        Output = coco_tool::UnstampedToolCallOutcome,
                                    > + Send,
                            >,
                        >
                })
            });
            let mut streaming_model_index: usize = 0;

            let api_start = std::time::Instant::now();
            // Half-open recovery probe: if a policy is configured
            // and the backoff window elapsed since the last
            // fallback switch, swap to primary for this turn. The
            // probe uses the same call path as a normal turn — no
            // side-channel ping — so success keeps the response
            // AND any cache-warming the provider did. Probe state
            // is owned by ModelRuntime (see `probe_in_flight`),
            // not here — the engine only decides when to start
            // and when to finalize.
            match model_runtime.attempt_probe_if_due(std::time::Instant::now()) {
                crate::model_runtime::ProbeDecision::Skip => {}
                crate::model_runtime::ProbeDecision::Probe => {
                    tracing::info!(
                        probe_target = model_runtime.current_model_name(),
                        "probing primary via half-open recovery",
                    );
                }
            }
            let was_probing = model_runtime.probe_in_flight();
            // Route through ModelRuntime so post-fallback / probe
            // calls reach the active provider. When no fallback is
            // configured this is identical to `self.client.query_stream`.
            let active_client = model_runtime.current_client();
            let mut rx = match active_client.query_stream(&params).await {
                Ok(rx) => {
                    // Success resets the capacity-error streak —
                    // isolated 529s must not accumulate across turns.
                    consecutive_capacity_errors = 0;
                    // Probe succeeded at stream-open — clear
                    // recovery state and announce the switch-back.
                    if was_probing {
                        let recovered = model_runtime.current_model_name().to_string();
                        model_runtime.finalize_probe(
                            crate::model_runtime::ProbeOutcome::Success,
                            std::time::Instant::now(),
                        );
                        emit_model_fallback_notice(
                            &event_tx,
                            /*original*/ "",
                            &recovered,
                            &self.config.session_id,
                            crate::model_runtime::ModelFallbackReason::ProbeRecovery,
                        )
                        .await;
                    }
                    rx
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    // Probe failure: transparently revert to the
                    // fallback, then retry the turn. A probe is
                    // OPTIONAL — failing one must NOT surface as
                    // a user-visible error; the session behaves
                    // exactly as if no probe had been attempted.
                    if was_probing {
                        model_runtime.finalize_probe(
                            crate::model_runtime::ProbeOutcome::Failure,
                            std::time::Instant::now(),
                        );
                        tracing::warn!(
                            active = model_runtime.current_model_name(),
                            error = %err_msg,
                            "probe failed at stream-open; reverting to fallback and retrying",
                        );
                        // Don't tick the capacity streak — probe
                        // and streak are independent signals.
                        // `continue` reruns the turn from the top
                        // using the reverted fallback slot.
                        continue;
                    }
                    if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                        warn!("prompt too long (stream open), attempting reactive compaction");
                        self.do_reactive_compact(&mut history, &event_tx).await;
                        last_continue_reason = Some(ContinueReason::ReactiveCompactRetry);
                        budget.reset_continuations();
                        continue;
                    }
                    if is_capacity_error_message(&err_msg) {
                        consecutive_capacity_errors += 1;
                        if consecutive_capacity_errors < MAX_CONSECUTIVE_CAPACITY_ERRORS {
                            // Below threshold: log and retry the
                            // turn on the same slot. The streak
                            // counter accumulates across
                            // iterations until `advance()` fires.
                            warn!(
                                consecutive = consecutive_capacity_errors,
                                threshold = MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                active = model_runtime.current_model_name(),
                                "capacity error below threshold; retrying on same slot",
                            );
                            continue;
                        }
                        if model_runtime.has_fallback() {
                            let original = model_runtime.current_model_name().to_string();
                            match model_runtime.advance() {
                                crate::model_runtime::AdvanceOutcome::Switched(new_model) => {
                                    warn!(
                                        original,
                                        fallback = new_model,
                                        consecutive = consecutive_capacity_errors,
                                        "advanced to next fallback slot after \
                                         capacity streak",
                                    );
                                    consecutive_capacity_errors = 0;
                                    emit_model_fallback_notice(
                                        &event_tx,
                                        &original,
                                        &new_model,
                                        &self.config.session_id,
                                        crate::model_runtime::ModelFallbackReason::CapacityDegrade {
                                            consecutive_errors: MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                        },
                                    )
                                    .await;
                                    continue;
                                }
                                crate::model_runtime::AdvanceOutcome::Exhausted => {
                                    warn!(
                                        active = original,
                                        "fallback chain exhausted on stream-open error",
                                    );
                                    emit_model_fallback_notice(
                                        &event_tx,
                                        &original,
                                        /*new_model*/ "",
                                        &self.config.session_id,
                                        crate::model_runtime::ModelFallbackReason::ChainExhausted,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    return Err(anyhow::anyhow!("LLM stream open failed: {e}"));
                }
            };

            // Accumulate stream state. `tool_order` preserves the order tool
            // calls first appeared (by `ToolInputStart`) so the downstream
            // exec path keeps the same ordering contract as the blocking path.
            //
            let mut response_text = String::new();
            let mut reasoning_text = String::new();
            let mut tool_order: Vec<String> = Vec::new();
            let mut tool_buffers: std::collections::HashMap<String, StreamingToolCallBuffer> =
                std::collections::HashMap::new();
            let mut stream_usage: Option<TokenUsage> = None;
            let mut stream_stop_reason: Option<String> = None;
            let mut stream_error: Option<String> = None;

            loop {
                let event = tokio::select! {
                    _ = self.cancel.cancelled() => {
                        // Cancellation mid-stream: drop the stream
                        // and fall through to the top-of-loop
                        // `is_cancelled()` check which returns a
                        // proper `Ok(QueryResult { cancelled: true })`.
                        // With streaming_tool_execution enabled, the
                        // StreamingHandle's JoinSet aborts any
                        // inflight safe tools when dropped
                        // (transitively via streaming_handle going
                        // out of scope as this function unwinds).
                        drop(rx);
                        break;
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
                        // Streaming mode: parse the freshly-completed
                        // input, run full per-tool preparation
                        // (validate → pre-hook → permission →
                        // re-validate), and feed the resulting plan
                        // to the StreamingHandle. Safe tools start
                        // executing immediately via tokio::spawn;
                        // unsafe tools queue for commit_flush.
                        //
                        // Errors from preparation (unknown tool,
                        // schema fail, permission deny, hook block)
                        // already push an error tool_result to
                        // history via the preparer's shared
                        // `complete_tool_call_with_error` helper, so
                        // the handle-path stays consistent without
                        // needing a separate fallback.
                        if let (Some(handle), Some(ctx_arc)) =
                            (streaming_handle.as_mut(), streaming_ctx.as_ref())
                            && let Some(buf) = tool_buffers.get(&id)
                            && buf.complete
                        {
                            let input: serde_json::Value = if buf.input_json.trim().is_empty() {
                                serde_json::Value::Object(Default::default())
                            } else {
                                match serde_json::from_str(&buf.input_json) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        warn!(
                                            tool_call_id = %id,
                                            tool_name = %buf.tool_name,
                                            error = %e,
                                            "streaming tool input JSON parse failed; dropping call"
                                        );
                                        continue;
                                    }
                                }
                            };
                            let tcp = ToolCallPart {
                                tool_call_id: id.clone(),
                                tool_name: buf.tool_name.clone(),
                                input,
                                provider_executed: None,
                                provider_metadata: None,
                            };
                            let slice = std::slice::from_ref(&tcp);
                            let mut prep_args = crate::tool_call_preparer::PendingToolPreparation {
                                event_tx: &event_tx,
                                history: &mut history,
                                ctx: ctx_arc.as_ref(),
                                tool_calls: slice,
                                tools: &self.tools,
                                hooks: self.hooks.as_ref(),
                                orchestration_ctx: self.orchestration_ctx(),
                                hook_tx_opt: hook_tx_opt.as_ref(),
                                permission_denials: &mut permission_denials,
                                state_tracker,
                                permission_bridge: self.permission_bridge.as_ref(),
                                session_id: &self.config.session_id,
                                cancel: &self.cancel,
                                auto_mode_state: self.auto_mode_state.as_ref(),
                                denial_tracker: self.denial_tracker.as_ref(),
                                client: &self.client,
                                auto_mode_rules: &self.auto_mode_rules,
                            };
                            if let Some((pending, _ctx)) =
                                crate::tool_call_preparer::prepare_one_pending_tool_call(
                                    &mut prep_args,
                                    &tcp,
                                )
                                .await
                            {
                                // Emit ToolUseStarted now that the
                                // call has passed pre-hook +
                                // permission and is about to be
                                // spawned. Non-streaming path emits
                                // this in tool_call_runner.rs:145;
                                // we mirror that here so SDK
                                // consumers see the same event
                                // sequence regardless of path.
                                let _ = emit_stream(
                                    &event_tx,
                                    crate::AgentStreamEvent::ToolUseStarted {
                                        call_id: pending.tool_use_id.clone(),
                                        name: pending.tool.name().to_string(),
                                        batch_id: None,
                                    },
                                )
                                .await;

                                let model_index = streaming_model_index;
                                streaming_model_index += 1;
                                handle.feed_plan(coco_tool::ToolCallPlan::Runnable(
                                    coco_tool::PreparedToolCall {
                                        tool_use_id: pending.tool_use_id,
                                        tool_id: pending.tool.id(),
                                        tool: pending.tool,
                                        parsed_input: pending.input,
                                        model_index,
                                    },
                                ));
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

            // Cancellation mid-stream: skip the rest of turn
            // processing and let the top-of-loop cancel check build
            // the proper `QueryResult { cancelled: true }`. Any
            // streaming handle in-flight is implicitly aborted when
            // this function unwinds (JoinSet drops cancel pending
            // tasks).
            if self.cancel.is_cancelled() {
                continue;
            }

            if let Some(err_msg) = stream_error {
                // Probe failure mid-stream: transparently revert
                // and retry — same rule as stream-open. Probes
                // are optional; their failures must never be
                // user-visible.
                if model_runtime.probe_in_flight() {
                    model_runtime.finalize_probe(
                        crate::model_runtime::ProbeOutcome::Failure,
                        std::time::Instant::now(),
                    );
                    tracing::warn!(
                        active = model_runtime.current_model_name(),
                        error = %err_msg,
                        "probe failed mid-stream; reverting to fallback and retrying",
                    );
                    continue;
                }
                if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                    warn!("prompt too long (stream), attempting reactive compaction");
                    self.do_reactive_compact(&mut history, &event_tx).await;
                    last_continue_reason = Some(ContinueReason::ReactiveCompactRetry);
                    budget.reset_continuations();
                    continue;
                }
                if is_capacity_error_message(&err_msg) {
                    consecutive_capacity_errors += 1;
                    if consecutive_capacity_errors < MAX_CONSECUTIVE_CAPACITY_ERRORS {
                        warn!(
                            consecutive = consecutive_capacity_errors,
                            threshold = MAX_CONSECUTIVE_CAPACITY_ERRORS,
                            active = model_runtime.current_model_name(),
                            "capacity error mid-stream below threshold; retrying on same slot",
                        );
                        continue;
                    }
                    if model_runtime.has_fallback() {
                        let original = model_runtime.current_model_name().to_string();
                        match model_runtime.advance() {
                            crate::model_runtime::AdvanceOutcome::Switched(new_model) => {
                                warn!(
                                    original,
                                    fallback = new_model,
                                    consecutive = consecutive_capacity_errors,
                                    "advanced to next fallback slot after \
                                     capacity streak (mid-stream)",
                                );
                                consecutive_capacity_errors = 0;
                                emit_model_fallback_notice(
                                    &event_tx,
                                    &original,
                                    &new_model,
                                    &self.config.session_id,
                                    crate::model_runtime::ModelFallbackReason::CapacityDegrade {
                                        consecutive_errors: MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                    },
                                )
                                .await;
                                continue;
                            }
                            crate::model_runtime::AdvanceOutcome::Exhausted => {
                                warn!(active = original, "fallback chain exhausted mid-stream",);
                                emit_model_fallback_notice(
                                    &event_tx,
                                    &original,
                                    /*new_model*/ "",
                                    &self.config.session_id,
                                    crate::model_runtime::ModelFallbackReason::ChainExhausted,
                                )
                                .await;
                            }
                        }
                    }
                }
                return Err(anyhow::anyhow!("LLM stream failed: {err_msg}"));
            }
            // Stream closed without error — reset the capacity streak
            // so an isolated failure followed by a successful turn
            // doesn't carry forward.
            consecutive_capacity_errors = 0;

            let usage = stream_usage.unwrap_or_default();
            total_usage += usage;
            budget.record_usage(&usage);
            // Record usage against the currently-active model id
            // (post-fallback value if a switch has happened).
            let model_id = model_runtime.current_model_name().to_string();
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

            // Streaming commit point: flush the StreamingHandle to
            // drain inflight safe tools, run queued unsafe tools
            // serially, apply patches in model-index order, and
            // push each outcome's ordered_messages into history.
            //
            // I12 note: outcomes surface in real completion order —
            // a slow earlier tool doesn't block a fast later one —
            // but `app_state_patch` apply is post-batch in
            // model-index order under one write lock (matches TS
            // `toolOrchestration.ts:54-62`).
            //
            // `streaming_executed` is the control-flow signal for
            // "this turn's tools all ran via streaming": we still
            // go through `finalize_turn_post_tools` + loop to the
            // next LLM call (unless the model produced no
            // tool_calls at all, in which case we fall through to
            // the `tool_calls.is_empty()` branch as before).
            let streaming_executed = streaming_ctx.is_some() && !tool_calls.is_empty();
            let mut streaming_control_prevent: Option<String> = None;
            // Collect ToolUseCompleted events to emit AFTER
            // commit_flush returns — the on_outcome callback is
            // synchronous (FnMut) and can't `.await`.
            let mut streaming_completed_events: Vec<(String, String, String, bool)> = Vec::new();
            if let Some(handle) = streaming_handle.take()
                && streaming_executed
            {
                let history_ref = &mut history;
                let prevent_slot = &mut streaming_control_prevent;
                let events_ref = &mut streaming_completed_events;
                handle
                    .commit_flush(0, |outcome| {
                        let call_id = outcome.tool_use_id().to_string();
                        let tool_name_str = outcome.tool_id().to_string();
                        let is_error = outcome.error_kind().is_some();
                        let output_text = extract_streaming_result_text(outcome.ordered_messages());
                        events_ref.push((call_id, tool_name_str, output_text, is_error));
                        if let Some(reason) = outcome.prevent_continuation()
                            && prevent_slot.is_none()
                        {
                            *prevent_slot = Some(reason.to_string());
                        }
                        let parts = outcome.into_parts();
                        for msg in parts.ordered_messages {
                            history_ref.push(msg);
                        }
                    })
                    .await;
            }
            for (call_id, tool_name, output, is_error) in streaming_completed_events {
                let _ = emit_stream(
                    &event_tx,
                    crate::AgentStreamEvent::ToolUseCompleted {
                        call_id,
                        name: tool_name,
                        output,
                        is_error,
                    },
                )
                .await;
            }

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

            // Streaming-executed fast path: the StreamingHandle
            // already ran every tool and pushed their
            // ordered_messages into history. Skip the non-streaming
            // runner, but still run finalize_turn_post_tools so
            // the command-queue drain / auto-compact / TurnCompleted
            // emission happens, then continue the loop.
            if streaming_executed {
                self.finalize_turn_post_tools(&mut history, &event_tx, turn_id, usage)
                    .await;
                if let Some(stop_reason) = streaming_control_prevent {
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
                        Some(stop_reason),
                        permission_denials,
                        history.messages.clone(),
                    ));
                }
                last_continue_reason = Some(ContinueReason::NextTurn);
                continue;
            }

            // Execute tool calls via StreamingToolExecutor (batch partitioning).
            // User-message id flows through the factory so the file-history
            // snapshot keys on the turn's triggering message, not a later
            // tool result. The factory installs a `QueryHookHandle` into
            // `ToolUseContext` when hooks are configured so tool callbacks
            // that need PreToolUse/PostToolUse use the same pipeline as the
            // runner.
            let ctx = self
                .tool_context_factory(hook_tx_opt.as_ref())
                .build(crate::tool_context::ToolContextOverrides {
                    user_message_id: Some(user_msg_uuid.clone()),
                    progress_tx: Some(progress_tx_session.clone()),
                    current_model_name: Some(model_runtime.current_model_name().to_string()),
                })
                .await;

            let tool_run_outcome = ToolCallRunner {
                event_tx: &event_tx,
                history: &mut history,
                ctx: &ctx,
                tool_calls: &tool_calls,
                turn,
                tools: &self.tools,
                hooks: self.hooks.as_ref(),
                orchestration_ctx: self.orchestration_ctx(),
                hook_tx_opt: hook_tx_opt.as_ref(),
                permission_denials: &mut permission_denials,
                state_tracker,
                permission_bridge: self.permission_bridge.as_ref(),
                session_id: &self.config.session_id,
                cancel: &self.cancel,
                auto_mode_state: self.auto_mode_state.as_ref(),
                denial_tracker: self.denial_tracker.as_ref(),
                client: &self.client,
                auto_mode_rules: &self.auto_mode_rules,
                app_state: self.app_state.as_ref(),
            }
            .run()
            .await;
            self.finalize_turn_post_tools(&mut history, &event_tx, turn_id, usage)
                .await;
            if !tool_run_outcome.continue_after_tools {
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
                    tool_run_outcome.stop_reason_override,
                    permission_denials,
                    history.messages.clone(),
                ));
            }
            last_continue_reason = Some(ContinueReason::NextTurn);
            let _ = tool_calls; // has_tool_calls retained for future metrics
        }
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
        //
        // TS `isAutoCompactEnabled()` short-circuits the trigger (not just
        // the reminder) — matching that parity keeps `auto_compact_enabled
        // = false` from silently rewriting history. Gate both call sites on
        // the same flag so a post-micro re-check stays consistent.
        let estimated_tokens = coco_compact::estimate_tokens(&history.messages);
        if self.config.auto_compact_enabled
            && coco_compact::should_auto_compact(
                estimated_tokens,
                self.config.context_window,
                self.config.max_output_tokens,
            )
        {
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
            if self.config.auto_compact_enabled
                && coco_compact::should_auto_compact(
                    post_micro_tokens,
                    self.config.context_window,
                    self.config.max_output_tokens,
                )
            {
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

                // TS: `createPlanAttachmentIfNeeded()` (`compact.ts:1470`)
                // — re-inject the plan file's content so it survives the
                // compaction boundary. Body uses the verbatim
                // `plan_file_reference` text template from
                // `messages.ts:3636-3642`.
                if let Some(ref ch) = config_home {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        project_dir.as_deref(),
                        plans_directory_setting.as_deref(),
                    );
                    let plan_path = coco_context::get_plan_file_path(
                        &session_id,
                        &plans_dir,
                        /*agent_id*/ None,
                    );
                    let plan_content =
                        coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
                    if let Some(att) = coco_compact::create_plan_attachment_if_needed(
                        &plan_path,
                        plan_content.as_deref(),
                    ) {
                        atts.push(att);
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
            attachment_emitter: self.attachment_emitter(),
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
    ///
    /// TS parity: each `Tool::prompt(&PromptOptions)` call returns the
    /// description the model sees that turn. Agent/Skill tools use
    /// this hook to inject live runtime state (current agent / skill
    /// listings) into their description. For tools that don't
    /// override `prompt`, the trait default delegates to
    /// `description()`, preserving the legacy behavior.
    async fn build_tool_definitions(
        &self,
        app_state: &ToolAppState,
    ) -> Vec<vercel_ai_provider::LanguageModelV4Tool> {
        let loaded = self.tools.loaded_tools();
        let tool_names: Vec<String> = loaded.iter().map(|t| t.name().to_string()).collect();

        let agent_names: Vec<String> = self
            .session_bootstrap
            .as_ref()
            .map(|b| b.agents.clone())
            .unwrap_or_default();
        let skill_names: Vec<String> = {
            let mut names = self
                .session_bootstrap
                .as_ref()
                .map(|b| b.skills.clone())
                .unwrap_or_default();
            names.sort();
            names
        };

        let permission_mode = app_state
            .permission_mode
            .unwrap_or(self.config.permission_mode);
        let permission_context = coco_types::ToolPermissionContext {
            mode: permission_mode,
            additional_dirs: std::collections::HashMap::new(),
            allow_rules: std::collections::HashMap::new(),
            deny_rules: std::collections::HashMap::new(),
            ask_rules: std::collections::HashMap::new(),
            bypass_available: self.config.bypass_permissions_available,
            pre_plan_mode: app_state.pre_plan_mode,
            stripped_dangerous_rules: app_state.stripped_dangerous_rules.clone(),
            session_plan_file: None,
        };
        let prompt_options = coco_tool::PromptOptions {
            is_non_interactive: self.config.is_non_interactive,
            tool_names,
            agent_names,
            allowed_agent_types: None,
            skill_names,
            permission_context: Some(permission_context),
        };

        let mut out = Vec::with_capacity(loaded.len());
        for tool in loaded {
            let schema = tool.input_schema();
            let json_schema = tool
                .input_json_schema()
                .unwrap_or_else(|| serde_json::to_value(&schema).unwrap_or_default());
            let description = tool.prompt(&prompt_options).await;
            out.push(LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
                name: tool.name().to_string(),
                description: Some(description),
                input_schema: json_schema,
                input_examples: None,
                strict: None,
                provider_options: None,
            }));
        }
        out
    }

    /// Build a factory that knows how to construct [`ToolUseContext`]
    /// snapshots from the engine's current config + shared handles.
    ///
    /// Each turn calls `factory.build(...)` to get a fresh context; the
    /// factory re-reads live `ToolAppState` per call so permission-mode
    /// mutations from a prior batch (e.g. `EnterPlanMode`) propagate
    /// without a config reload.
    ///
    /// The field mapping itself — including the five previously-hardcoded
    /// fields (`thinking_level`, `is_non_interactive`, `max_budget_usd`,
    /// `custom_system_prompt`, `append_system_prompt`) — is verified in
    /// `tool_context.test.rs`.
    fn tool_context_factory(
        &self,
        hook_tx: Option<&tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
    ) -> crate::tool_context::ToolContextFactory {
        // Build the structured hook handle here — every tool call built
        // through this factory gets the same `QueryHookHandle`, so
        // PreToolUse/PostToolUse/PostToolUseFailure fire consistently
        // regardless of the call site. When the session has no hook
        // registry (tests / single-turn helpers) we pass `None` and the
        // `ToolUseContext` receives no handle — executor treats that as
        // a no-op, matching legacy behavior.
        let hook_handle = self.hooks.as_ref().map(|registry| {
            let handle: coco_tool::HookHandleRef =
                std::sync::Arc::new(crate::hook_adapter::QueryHookHandle::new(
                    registry.clone(),
                    self.orchestration_ctx(),
                    hook_tx.cloned(),
                ));
            handle
        });
        crate::tool_context::ToolContextFactory {
            config: self.config.clone(),
            tools: self.tools.clone(),
            cancel: self.cancel.clone(),
            mailbox: self.mailbox.clone(),
            task_list: self.task_list.clone(),
            todo_list: self.todo_list.clone(),
            permission_bridge: self.permission_bridge.clone(),
            app_state: self.app_state.clone(),
            file_read_state: self.file_read_state.clone(),
            file_history: self.file_history.clone(),
            config_home: self.config_home.clone(),
            hook_handle,
            // Real `AgentHandle` when the CLI / SDK / TUI installed
            // one via `with_agent_handle`; otherwise fall back to
            // `NoOpAgentHandle`.
            agent_handle: self.agent_handle.clone(),
            // `SkillHandle` same pattern — real handle when
            // installed, `NoOpSkillHandle` otherwise.
            skill_handle: self.skill_handle.clone(),
            // Session-scoped schema validator. Clone is cheap —
            // inner state is `Arc<RwLock<HashMap>>` shared across
            // per-turn ctx rebuilds so the compile cache persists.
            tool_schema_validator: Some(self.tool_schema_validator.clone()),
        }
    }

    /// Detect local-date rollover for the `date_change` system reminder.
    ///
    /// Reads `ToolAppState::last_emitted_date`, compares it to today's
    /// local ISO date, and:
    ///
    /// - seeds the latch on first observation, returning `None`
    ///   (no reminder — TS `getDateChangeAttachments` matches: the first
    ///   turn of a session never emits because there's no prior date);
    /// - returns `Some(today)` and updates the latch on a mismatch
    ///   (engine passes it to `TurnReminderInput.new_date` and the
    ///   `DateChangeGenerator` emits once);
    /// - returns `None` when the latch already matches today.
    ///
    /// No-op (returns `None`) when `self.app_state` is `None`.
    async fn observe_date_change(&self) -> Option<String> {
        let state = self.app_state.as_ref()?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut guard = state.write().await;
        match guard.last_emitted_date.as_deref() {
            Some(prev) if prev == today => None,
            Some(_) => {
                guard.last_emitted_date = Some(today.clone());
                Some(today)
            }
            None => {
                // First observation: seed without emitting.
                guard.last_emitted_date = Some(today);
                None
            }
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

/// Compute the TS-parity `deferred_tools_delta` between the current tool
/// set and the last-announced set stored on `ToolAppState`.
///
/// Returns `None` when the sets are equal (nothing to announce); returns
/// `Some(info)` with `added_lines` / `removed_names` when they differ.
///
/// TS `getDeferredToolsDelta` at `attachments.ts:1472` reconstructs the
/// announced set by scanning history for prior delta attachments;
/// coco-rs persists the set directly on app_state, so this diff is
/// O(|current ∪ announced|).
/// Extract the first `ToolResult` text payload from a run of
/// `ordered_messages` so the streaming path can populate
/// `ToolUseCompleted.output` with the same string the SDK expects.
/// Mirrors the non-streaming runner's `render_completed_output`
/// helper in `tool_call_runner.rs`.
/// LRU + time-window throttle for protocol-level tool-progress events.
///
/// TS parity: `utils/queryHelpers.ts:99-188` — one throttle per
/// `parent_tool_use_id`, ≤1 emission / 30 s, LRU-bound to 100 keys.
pub(crate) struct ProgressThrottle {
    last_sent: std::collections::HashMap<String, std::time::Instant>,
    throttle: std::time::Duration,
    max_tracking: usize,
}

impl ProgressThrottle {
    /// Matches TS defaults (30 s window, 100-key LRU).
    pub(crate) fn new() -> Self {
        Self::with_params(std::time::Duration::from_secs(30), 100)
    }

    /// Test-only constructor that takes an explicit window + LRU
    /// size. The tests use a 1 ms window so they don't need to sleep.
    pub(crate) fn with_params(throttle: std::time::Duration, max_tracking: usize) -> Self {
        Self {
            last_sent: std::collections::HashMap::new(),
            throttle,
            max_tracking,
        }
    }

    /// Returns `true` if a protocol event for `key` should be
    /// emitted now and stamps the send time. Returns `false` (skip)
    /// when a prior emission fell inside the throttle window.
    pub(crate) fn allow(&mut self, key: &str, now: std::time::Instant) -> bool {
        if let Some(prev) = self.last_sent.get(key)
            && now.duration_since(*prev) < self.throttle
        {
            return false;
        }
        if self.last_sent.len() >= self.max_tracking
            && let Some(oldest) = self
                .last_sent
                .iter()
                .min_by_key(|&(_, t)| *t)
                .map(|(k, _)| k.clone())
        {
            self.last_sent.remove(&oldest);
        }
        self.last_sent.insert(key.to_string(), now);
        true
    }
}

/// Extract `(tool_name, elapsed_seconds, task_id)` from a
/// `ToolProgress.data` payload IF it matches a TS-parity
/// bash/powershell shape. Returns `None` for unrelated payload
/// types (e.g. agent/skill progress) — those follow different
/// propagation rules and are not currently surfaced as
/// `ServerNotification::ToolProgress`.
pub(crate) fn classify_progress_payload(
    data: &serde_json::Value,
) -> Option<(&'static str, f64, Option<String>)> {
    let obj = data.as_object()?;
    let ptype = obj.get("type").and_then(serde_json::Value::as_str)?;
    let tool_name = match ptype {
        "bash_progress" => "Bash",
        "powershell_progress" => "PowerShell",
        _ => return None,
    };
    let elapsed = obj
        .get("elapsedTimeSeconds")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let task_id = obj
        .get("taskId")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
    Some((tool_name, elapsed, task_id))
}

/// Fan out a single `ToolProgress` event to both TUI and protocol
/// layers, applying the throttle to the protocol layer. Extracted
/// from the session drain loop so it can be unit-tested without
/// standing up a full engine.
pub(crate) async fn drain_one_progress(
    event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    progress: coco_tool::ToolProgress,
    throttle: &mut ProgressThrottle,
) {
    // Fan-out #1: raw TUI event, always emitted.
    let tool_use_id = progress.tool_use_id.clone();
    let _ = crate::emit::emit_tui(
        event_tx,
        coco_types::TuiOnlyEvent::ToolProgress {
            tool_use_id: tool_use_id.clone(),
            data: progress.data.clone(),
        },
    )
    .await;

    // Fan-out #2: protocol ToolProgress. Only bash/powershell
    // progress qualifies (TS `queryHelpers.ts:158-199`).
    let Some((tool_name, elapsed, task_id)) = classify_progress_payload(&progress.data) else {
        return;
    };

    // Throttle key: TS uses `parentToolUseID` because `toolUseID`
    // rotates per progress event in its world. Rust's tool_use_id
    // is stable, so it's a safe fallback when parent is absent.
    let key = progress
        .parent_tool_use_id
        .clone()
        .unwrap_or_else(|| tool_use_id.clone());
    if !throttle.allow(&key, std::time::Instant::now()) {
        return;
    }

    let _ = crate::emit::emit_protocol(
        event_tx,
        coco_types::ServerNotification::ToolProgress(coco_types::ToolProgressParams {
            tool_use_id,
            tool_name: tool_name.to_string(),
            parent_tool_use_id: progress.parent_tool_use_id,
            elapsed_time_seconds: elapsed,
            task_id,
        }),
    )
    .await;
}

/// Classify an error message as a transient capacity error.
///
/// TS parity: `is529Error` + 429 clauses in `services/api/withRetry.ts`.
/// Rust's [`coco_inference::InferenceError::Overloaded`] Display formats
/// as `"provider overloaded"`; rate-limit as `"rate limited"`. Raw HTTP
/// status codes appear in messages bubbled from provider crates. Match
/// any of these.
fn is_capacity_error_message(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("provider overloaded")
        || m.contains("overloaded_error")
        || m.contains("rate limited")
        || m.contains("rate_limit")
        || m.contains("status: 529")
        || m.contains("status: 503")
        || m.contains("(529)")
        || m.contains("(503)")
}

/// Announce a model fallback / recovery transition as an inline
/// stream notice. TS parity: `query.ts:946` writes a system-tagged
/// line into the transcript so SDK consumers + the TUI see it
/// alongside the agent's response.
///
/// Templates are direction-aware:
/// - `CapacityDegrade` → "Switched to {new} due to high demand for {original}."
/// - `ProbeRecovery`   → "Recovered to primary {new} after probe."
/// - `ChainExhausted`  → "All provider slots exhausted (last tried: {original})."
///
/// `original` may be empty if the previous slot never identified
/// itself — the message degrades gracefully on each branch.
async fn emit_model_fallback_notice(
    event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
    original: &str,
    new_model: &str,
    session_id: &str,
    reason: crate::model_runtime::ModelFallbackReason,
) {
    use crate::model_runtime::ModelFallbackReason;
    let notice = match reason {
        ModelFallbackReason::CapacityDegrade { .. } => {
            if original.is_empty() {
                format!("[system] Switched to fallback model {new_model} due to high demand.\n")
            } else {
                format!("[system] Switched to {new_model} due to high demand for {original}.\n")
            }
        }
        ModelFallbackReason::ProbeRecovery => {
            format!("[system] Recovered to primary {new_model} after probe.\n")
        }
        ModelFallbackReason::ChainExhausted => {
            if original.is_empty() {
                "[system] All provider slots exhausted.\n".to_string()
            } else {
                format!("[system] All provider slots exhausted (last tried: {original}).\n")
            }
        }
    };
    let _ = crate::emit::emit_stream(
        event_tx,
        crate::AgentStreamEvent::TextDelta {
            turn_id: session_id.to_string(),
            delta: notice,
        },
    )
    .await;
}

fn extract_streaming_result_text(ordered: &[Message]) -> String {
    for msg in ordered {
        if let Message::ToolResult(tr) = msg
            && let coco_types::LlmMessage::Tool { content, .. } = &tr.message
        {
            for part in content {
                if let coco_types::ToolContent::ToolResult(r) = part {
                    match &r.output {
                        vercel_ai_provider::ToolResultContent::Text { value, .. } => {
                            return value.clone();
                        }
                        vercel_ai_provider::ToolResultContent::ErrorText { value, .. } => {
                            return value.clone();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    String::new()
}

fn compute_tools_delta(
    current_tools: &[String],
    last_announced: &std::collections::HashSet<String>,
) -> Option<coco_system_reminder::DeferredToolsDeltaInfo> {
    let current_set: std::collections::HashSet<&String> = current_tools.iter().collect();

    let mut added_lines: Vec<String> = current_tools
        .iter()
        .filter(|t| !last_announced.contains(t.as_str()))
        .map(|t| format!("- {t}"))
        .collect();
    let mut removed_names: Vec<String> = last_announced
        .iter()
        .filter(|t| !current_set.contains(*t))
        .cloned()
        .collect();

    if added_lines.is_empty() && removed_names.is_empty() {
        return None;
    }
    // Stable ordering so consecutive emissions with the same delta
    // produce byte-identical reminders (simpler to diff in tests + logs).
    added_lines.sort();
    removed_names.sort();
    Some(coco_system_reminder::DeferredToolsDeltaInfo {
        added_lines,
        removed_names,
    })
}

/// Extract the raw user-input text from the most-recent non-meta user
/// message in history. Mirrors TS `getAttachments(input, ...)` where
/// `input` is the user's prompt string (not a structured message).
/// Returns `None` when there's no plain-text user message (e.g. the
/// session opened with a compacted summary).
fn latest_user_input_text(history: &coco_messages::MessageHistory) -> Option<String> {
    for msg in history.messages.iter().rev() {
        let coco_types::Message::User(u) = msg else {
            continue;
        };
        if let coco_types::LlmMessage::User { content, .. } = &u.message {
            for part in content {
                if let vercel_ai_provider::UserContentPart::Text(tp) = part {
                    return Some(tp.text.clone());
                }
            }
        }
    }
    None
}

/// Compute the TS-parity `mcp_instructions_delta` between the current
/// server-instruction set and the last-announced set on `ToolAppState`.
///
/// TS: `getMcpInstructionsDeltaAttachment` reconstructs the announced
/// set by scanning prior delta attachments in history; coco-rs
/// persists the announced map on `app_state.last_announced_mcp_instructions`
/// so the diff is O(|current ∪ announced|).
fn compute_mcp_instructions_delta(
    current: &std::collections::HashMap<String, String>,
    last_announced: &std::collections::HashMap<String, String>,
) -> Option<coco_system_reminder::McpInstructionsDeltaInfo> {
    let mut added_blocks: Vec<String> = current
        .iter()
        .filter(|(name, text)| {
            last_announced
                .get(name.as_str())
                .is_none_or(|prev| prev != *text)
        })
        .map(|(name, text)| format!("## {name}\n\n{text}"))
        .collect();
    let mut removed_names: Vec<String> = last_announced
        .keys()
        .filter(|name| !current.contains_key(name.as_str()))
        .cloned()
        .collect();

    if added_blocks.is_empty() && removed_names.is_empty() {
        return None;
    }
    added_blocks.sort();
    removed_names.sort();
    Some(coco_system_reminder::McpInstructionsDeltaInfo {
        added_blocks,
        removed_names,
    })
}

/// Compute the TS-parity `agent_listing_delta` between the current agent
/// types and the last-announced set on `ToolAppState`. `is_initial` is
/// true when no agents have been announced yet (first emission of the
/// session); that flips the TS "Available agent types" header (vs
/// "New agent types are now available").
fn compute_agents_delta(
    current_agents: &[String],
    last_announced: &std::collections::HashSet<String>,
) -> Option<coco_system_reminder::AgentListingDeltaInfo> {
    let current_set: std::collections::HashSet<&String> = current_agents.iter().collect();

    let mut added_lines: Vec<String> = current_agents
        .iter()
        .filter(|t| !last_announced.contains(t.as_str()))
        .map(|t| format!("- {t}"))
        .collect();
    let mut removed_types: Vec<String> = last_announced
        .iter()
        .filter(|t| !current_set.contains(*t))
        .cloned()
        .collect();

    if added_lines.is_empty() && removed_types.is_empty() {
        return None;
    }
    added_lines.sort();
    removed_types.sort();
    let is_initial = last_announced.is_empty();
    Some(coco_system_reminder::AgentListingDeltaInfo {
        added_lines,
        removed_types,
        is_initial,
        show_concurrency_note: is_initial,
    })
}

#[cfg(test)]
#[path = "engine.test.rs"]
mod tests;
