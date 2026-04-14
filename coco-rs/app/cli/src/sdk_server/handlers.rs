//! Per-method handlers for the SDK server dispatch loop.
//!
//! Each `ClientRequest` variant is routed to a handler function that
//! returns a `HandlerResult`. Handlers have access to a `HandlerContext`
//! carrying the notification channel (for emitting progress events
//! mid-handler) and any per-session state.
//!
//! This file is the central dispatch point — specific handlers live as
//! `handle_*` functions below (Phase 2.C.2 initialize/session, 2.C.3
//! turn, 2.C.4 approval + user input). Phase 2.C.1 laid the structural
//! groundwork with a match statement that is exhaustive: adding a new
//! `ClientRequest` variant will fail compilation here, forcing a
//! handler to be written.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use coco_types::ApprovalResolveParams;
use coco_types::ClientHookCallbackResponseParams;
use coco_types::ClientRequest;
use coco_types::CoreEvent;
use coco_types::ElicitationResolveParams;
use coco_types::InitializeResult;
use coco_types::JsonRpcMessage;
use coco_types::JsonRpcRequest;
use coco_types::McpRouteMessageResponseParams;
use coco_types::RequestId;
use coco_types::SdkModelInfo;
use coco_types::SessionStartResult;
use coco_types::TurnStartParams;
use coco_types::UserInputResolveParams;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::sdk_server::transport::SdkTransport;

/// The SDK protocol version coco-rs speaks.
pub const PROTOCOL_VERSION: &str = "1.0";

/// RAII cleanup for a pending `send_server_request` entry.
///
/// The `send_server_request` function registers a oneshot sender in
/// `SdkServerState.pending_server_requests` before writing the request
/// to the transport. On the happy path, `resolve_server_request` removes
/// the entry when the reply arrives. On the cancelled path (e.g. caller
/// wraps the await in `tokio::select!` with a cancel token and the cancel
/// branch fires), the future is dropped mid-await — without this guard,
/// the entry would leak in the HashMap until state drop.
///
/// The guard holds a reference to the pending map and uses synchronous
/// `try_lock` in its `Drop` impl. If the mutex is contended at drop time
/// (another task is mid-write), the entry leaks — but that's a very
/// narrow window and the leak is bounded by concurrency.
struct PendingRequestGuard<'a> {
    map: &'a Mutex<HashMap<RequestId, oneshot::Sender<JsonRpcMessage>>>,
    request_id: RequestId,
    /// Set to `false` after `resolve_server_request` has already
    /// removed the entry (i.e. the happy-path Ok(reply) return).
    active: bool,
}

impl Drop for PendingRequestGuard<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut map) = self.map.try_lock() {
            map.remove(&self.request_id);
        }
        // If try_lock fails, accept the leak. It's bounded and will be
        // reclaimed when SdkServerState is dropped.
    }
}

// ---------------------------------------------------------------------------
// TurnRunner — abstracts over "how to run a turn"
// ---------------------------------------------------------------------------

/// Boxed future used by trait methods.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Abstraction over how the SDK server executes a single turn.
///
/// The sdk_server module doesn't depend on `coco-query`, so the dispatch
/// layer stays pure. In production, the CLI entry point wires a concrete
/// runner that spawns a `QueryEngine`. Tests inject mock runners that
/// emit scripted events.
pub trait TurnRunner: Send + Sync {
    /// Run a single turn.
    ///
    /// - `params`: the `turn/start` parameters from the client.
    /// - `session`: the active session this turn belongs to.
    /// - `event_tx`: the channel on which CoreEvents must be emitted.
    ///   The dispatcher's notification forwarder reads from this channel
    ///   and writes JsonRpc notifications to the transport.
    /// - `cancel`: cancellation token. `turn/interrupt` triggers this.
    ///
    /// Returning `Ok(())` signals a clean turn completion. Returning an
    /// error causes the server to emit a `turn/failed` notification (future)
    /// and log the error.
    fn run_turn<'a>(
        &'a self,
        params: TurnStartParams,
        session: SessionHandle,
        event_tx: mpsc::Sender<CoreEvent>,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
}

/// Default runner used when no runner is injected. Returns an error
/// indicating that the server was not configured with a real runner.
pub struct NotImplementedRunner;

impl TurnRunner for NotImplementedRunner {
    fn run_turn<'a>(
        &'a self,
        _params: TurnStartParams,
        _session: SessionHandle,
        _event_tx: mpsc::Sender<CoreEvent>,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            anyhow::bail!(
                "SdkServer was constructed without a TurnRunner; \
                 call SdkServer::with_turn_runner() before run()"
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Server + session state
// ---------------------------------------------------------------------------

/// Shared server state carried across ClientRequests within a single
/// stdio session. Only one concurrent session per server — mirrors TS
/// where `structuredIO.ts` holds a single `currentSession` slot.
pub struct SdkServerState {
    /// Active session if any. Set by `session/start`, cleared by
    /// `session/archive` or when the transport closes.
    pub session: RwLock<Option<SessionHandle>>,
    /// The runner that executes turns. Defaulted to `NotImplementedRunner`.
    /// Stored behind `RwLock` so `SdkServer::set_turn_runner()` can
    /// install a real runner after the state is already shared (used
    /// by the approval-bridge wiring path where the bridge needs a
    /// reference to the live state before the runner is constructed).
    pub turn_runner: RwLock<Arc<dyn TurnRunner>>,
    /// Pending `approval/askForApproval` ServerRequests awaiting a client
    /// `approval/resolve`. Keyed by `request_id`; value is the oneshot
    /// sender the agent-side code is awaiting.
    pub pending_approvals: Mutex<HashMap<String, oneshot::Sender<ApprovalResolveParams>>>,
    /// Pending `input/requestUserInput` ServerRequests awaiting a client
    /// `input/resolveUserInput`. Same correlation shape as approvals.
    pub pending_user_input: Mutex<HashMap<String, oneshot::Sender<UserInputResolveParams>>>,
    /// Pending `hook/callback` ServerRequests awaiting a client
    /// `hook/callbackResponse`. Keyed by `callback_id`.
    pub pending_hook_callbacks:
        Mutex<HashMap<String, oneshot::Sender<ClientHookCallbackResponseParams>>>,
    /// Pending `mcp/routeMessage` ServerRequests awaiting a client
    /// `mcp/routeMessageResponse`. Keyed by `request_id`.
    pub pending_mcp_routes: Mutex<HashMap<String, oneshot::Sender<McpRouteMessageResponseParams>>>,
    /// Pending elicitation ServerRequests awaiting a client
    /// `elicitation/resolve`. Keyed by `request_id`.
    pub pending_elicitations: Mutex<HashMap<String, oneshot::Sender<ElicitationResolveParams>>>,
    /// Pending ServerRequests (server→client) awaiting a
    /// `JsonRpcMessage::Response` or `JsonRpcMessage::Error` reply.
    /// Keyed by the server-issued `RequestId`.
    ///
    /// Populated by [`SdkServerState::send_server_request`] when an
    /// outbound request is written to the transport; drained by the
    /// dispatcher's `handle_message` when the matching response arrives.
    pub pending_server_requests: Mutex<HashMap<RequestId, oneshot::Sender<JsonRpcMessage>>>,
    /// Monotonic counter for issuing unique request IDs for outbound
    /// ServerRequests. Uses negative integers to avoid colliding with
    /// client-issued IDs (which are typically non-negative).
    pub next_server_request_id: AtomicI64,
    /// Transport handle shared with the dispatcher. Populated by
    /// `SdkServer::run()` at startup; used by the approval bridge and
    /// other ServerRequest-emitting code paths. `None` in tests that
    /// construct `SdkServerState` directly.
    pub transport: RwLock<Option<Arc<dyn SdkTransport>>>,
    /// Optional disk-backed [`coco_session::SessionManager`] used by
    /// the `session/list`, `session/read`, `session/resume` handlers
    /// to browse and restore historical sessions. When `None`, those
    /// handlers reply with `METHOD_NOT_FOUND` (session persistence is
    /// disabled). The CLI entry point (`run_sdk_mode`) wires one
    /// pointing at `~/.coco/sessions`; in-memory tests that don't
    /// exercise session/list can leave it as `None`.
    pub session_manager: RwLock<Option<Arc<coco_session::SessionManager>>>,
    /// Optional shared file-history state used by `control/rewindFiles`.
    /// When `None`, that handler errors with `INVALID_REQUEST`
    /// ("file history not enabled"). The CLI entry point wires a
    /// fresh `FileHistoryState` at startup; tests that don't exercise
    /// rewind can leave it as `None`.
    pub file_history: RwLock<Option<Arc<RwLock<coco_context::FileHistoryState>>>>,
    /// Config home directory used for file-history backups (resolved
    /// from `coco_config::global_config::config_home()` at CLI startup).
    /// Used in conjunction with `file_history` above.
    pub file_history_config_home: RwLock<Option<std::path::PathBuf>>,
    /// Optional MCP connection manager used by the `mcp/setServers`,
    /// `mcp/reconnect`, `mcp/toggle` handlers. The manager is wrapped
    /// in `tokio::sync::Mutex` (not `RwLock`) because `register_server`
    /// requires `&mut self` while `connect`/`disconnect` only need
    /// `&self`. The Mutex serializes both kinds of access — fine for
    /// these infrequent runtime-control operations.
    ///
    /// When `None`, the MCP lifecycle handlers respond with
    /// `INVALID_REQUEST` ("MCP manager not enabled").
    pub mcp_manager: RwLock<Option<Arc<Mutex<coco_mcp::McpConnectionManager>>>>,
}

impl Default for SdkServerState {
    fn default() -> Self {
        Self {
            session: RwLock::new(None),
            turn_runner: RwLock::new(Arc::new(NotImplementedRunner) as Arc<dyn TurnRunner>),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_user_input: Mutex::new(HashMap::new()),
            pending_hook_callbacks: Mutex::new(HashMap::new()),
            pending_mcp_routes: Mutex::new(HashMap::new()),
            pending_elicitations: Mutex::new(HashMap::new()),
            pending_server_requests: Mutex::new(HashMap::new()),
            // Start at -1 and decrement. Keeps us out of the typical
            // client-issued integer range and makes outbound IDs
            // visually distinctive in logs.
            next_server_request_id: AtomicI64::new(-1),
            transport: RwLock::new(None),
            session_manager: RwLock::new(None),
            file_history: RwLock::new(None),
            file_history_config_home: RwLock::new(None),
            mcp_manager: RwLock::new(None),
        }
    }
}

impl SdkServerState {
    /// Register an expected `approval/resolve`. Returns the receiver the
    /// agent-side code should `await` to get the client's decision.
    ///
    /// Callers are responsible for sending the matching `AskForApproval`
    /// ServerRequest to the client; on the response path, the dispatcher
    /// will wake this receiver via [`Self::resolve_approval`].
    pub async fn register_approval(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<ApprovalResolveParams> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.pending_approvals.lock().await;
        map.insert(request_id, tx);
        rx
    }

    /// Register an expected `input/resolveUserInput`. Mirror of
    /// [`Self::register_approval`] for the `RequestUserInput` flow.
    pub async fn register_user_input(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<UserInputResolveParams> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.pending_user_input.lock().await;
        map.insert(request_id, tx);
        rx
    }

    /// Register an expected `hook/callbackResponse`. Mirror of
    /// [`Self::register_approval`] for the `HookCallback` flow.
    pub async fn register_hook_callback(
        &self,
        callback_id: String,
    ) -> oneshot::Receiver<ClientHookCallbackResponseParams> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.pending_hook_callbacks.lock().await;
        map.insert(callback_id, tx);
        rx
    }

    /// Register an expected `mcp/routeMessageResponse`. Mirror of
    /// [`Self::register_approval`] for the `McpRouteMessage` flow.
    pub async fn register_mcp_route(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<McpRouteMessageResponseParams> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.pending_mcp_routes.lock().await;
        map.insert(request_id, tx);
        rx
    }

    /// Register an expected `elicitation/resolve`. Mirror of
    /// [`Self::register_approval`] for the MCP elicitation flow —
    /// used when an MCP server sends an elicitation request to the
    /// agent, which then forwards it to the SDK client.
    pub async fn register_elicitation(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<ElicitationResolveParams> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.pending_elicitations.lock().await;
        map.insert(request_id, tx);
        rx
    }

    /// Issue an outbound ServerRequest on the provided transport and
    /// await the matching response.
    ///
    /// Generates a fresh monotonically-decreasing `RequestId` (starting
    /// at -1), registers an oneshot in `pending_server_requests`, writes
    /// the `JsonRpcRequest` onto the transport, and awaits the receiver.
    /// The dispatcher's inbound-message handler wakes the receiver when
    /// the client replies with a matching `Response`/`Error`.
    ///
    /// Returns:
    /// - `Ok(JsonRpcMessage::Response(r))` — client replied successfully
    /// - `Ok(JsonRpcMessage::Error(e))` — client replied with an error
    /// - `Err(...)` — transport send failed or the oneshot was dropped
    ///   (e.g. the transport closed before the client replied)
    pub async fn send_server_request(
        &self,
        transport: &Arc<dyn SdkTransport>,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<JsonRpcMessage> {
        // Allocate a fresh id.
        let raw = self.next_server_request_id.fetch_sub(1, Ordering::SeqCst);
        let request_id = RequestId::Integer(raw);

        // Register a pending slot BEFORE sending so the response can't
        // race ahead of the insert.
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending_server_requests.lock().await;
            map.insert(request_id.clone(), tx);
        }

        // Drop-guard to clean up the pending slot if this future is
        // dropped before `rx.await` completes normally — e.g. the
        // caller wrapped us in a `tokio::select!` and the cancel branch
        // fired. Without this, cancelled approvals would leak an entry
        // in `pending_server_requests` until state drop (see third-round
        // review Fix #L).
        //
        // The guard uses `try_lock` in its sync `Drop` impl. If the
        // mutex is contended at drop time, the entry leaks — but that's
        // a very narrow window (only contended while another caller is
        // reading/writing the map), and the leak is bounded.
        let mut pending_guard = PendingRequestGuard {
            map: &self.pending_server_requests,
            request_id: request_id.clone(),
            active: true,
        };

        // Write the request onto the transport.
        let req = JsonRpcRequest {
            request_id: request_id.clone(),
            method: method.into(),
            params,
        };
        let msg = JsonRpcMessage::Request(req);
        if let Err(e) = transport.send(msg).await {
            // Guard will clean up on drop.
            anyhow::bail!("failed to send server request: {e}");
        }

        // Await the client's reply. If the sender is dropped
        // (e.g. transport closed), RecvError propagates.
        match rx.await {
            Ok(reply) => {
                // `resolve_server_request` already removed the entry
                // from the map when it delivered the reply. Tell the
                // guard to skip its cleanup on drop.
                pending_guard.active = false;
                Ok(reply)
            }
            Err(_) => {
                // Sender dropped without a reply — treat as cancelled.
                // Guard will clean up.
                anyhow::bail!("server request {raw} cancelled: no reply received")
            }
        }
    }

    /// Deliver an inbound `Response`/`Error` to the pending server
    /// request with the matching `request_id`, if any. Called by the
    /// dispatcher when it reads a message from the transport.
    ///
    /// Returns `true` if the message was routed to a pending request;
    /// `false` if no match was found (the client is replying to a
    /// request we don't have — usually a protocol confusion, logged
    /// but not fatal).
    pub async fn resolve_server_request(&self, msg: JsonRpcMessage) -> bool {
        let request_id = match &msg {
            JsonRpcMessage::Response(r) => r.request_id.clone(),
            JsonRpcMessage::Error(e) => e.request_id.clone(),
            _ => return false,
        };
        let mut map = self.pending_server_requests.lock().await;
        let Some(sender) = map.remove(&request_id) else {
            debug!(
                request_id = %request_id.as_display(),
                "resolve_server_request: no pending match"
            );
            return false;
        };
        // If the agent-side receiver has been dropped, the client's
        // reply is effectively lost. Log and move on.
        if sender.send(msg).is_err() {
            warn!(
                request_id = %request_id.as_display(),
                "resolve_server_request: receiver dropped before reply arrived"
            );
        }
        true
    }
}

impl std::fmt::Debug for SdkServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkServerState")
            .field("session", &"RwLock<..>")
            .field("turn_runner", &"RwLock<Arc<dyn TurnRunner>>")
            .field("pending_approvals", &"Mutex<HashMap<..>>")
            .field("pending_user_input", &"Mutex<HashMap<..>>")
            .field("pending_hook_callbacks", &"Mutex<HashMap<..>>")
            .field("pending_mcp_routes", &"Mutex<HashMap<..>>")
            .field("pending_elicitations", &"Mutex<HashMap<..>>")
            .field("pending_server_requests", &"Mutex<HashMap<..>>")
            .field(
                "next_server_request_id",
                &self.next_server_request_id.load(Ordering::Relaxed),
            )
            .field("session_manager", &"RwLock<Option<Arc<SessionManager>>>")
            .field(
                "file_history",
                &"RwLock<Option<Arc<RwLock<FileHistoryState>>>>",
            )
            .field("file_history_config_home", &"RwLock<Option<PathBuf>>")
            .field(
                "mcp_manager",
                &"RwLock<Option<Arc<Mutex<McpConnectionManager>>>>",
            )
            .finish()
    }
}

/// Handle for an active SDK session.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    /// Session-scoped permission mode override. When `None`, turns use
    /// the CLI-default mode; `control/setPermissionMode` sets this.
    pub permission_mode: Option<coco_types::PermissionMode>,
    /// Session-scoped thinking level override.
    /// `control/setThinking` sets this.
    pub thinking_level: Option<coco_types::ThinkingLevel>,
    /// Session-scoped environment variable overrides. Mutated by
    /// `control/updateEnv`; applied to tool invocations when wired.
    pub env_overrides: std::collections::HashMap<String, String>,
    /// Cancel token for the currently-running turn (if any).
    /// Set by `turn/start`, cleared when the turn completes.
    /// `turn/interrupt` looks this up and calls `.cancel()`.
    pub active_turn_cancel: Option<CancellationToken>,
    /// Monotonic counter for issuing turn IDs within this session.
    pub turn_counter: i32,
    /// Wall-clock timestamp when the session was created (for duration_ms).
    pub started_at: std::time::Instant,
    /// Per-session aggregated stats accumulated across every `turn/start`.
    /// Populated by the event forwarder when it intercepts per-turn
    /// `SessionResult` notifications from the engine. Emitted back to the
    /// client as a single `SessionResult` when the session is archived.
    pub stats: SessionStats,
    /// Cumulative message history across every `turn/start` in this
    /// session. Used by `QueryEngineRunner` to thread context between
    /// turns: the runner locks this, builds combined messages
    /// `prior_history + [new_user_msg]`, calls
    /// `QueryEngine::run_with_messages`, then replaces the contents
    /// with `QueryResult.final_messages`. The `Arc<Mutex<>>` wrapping
    /// lets the runner's detached turn task mutate it without holding
    /// the session write-lock for the whole turn.
    pub history: Arc<Mutex<Vec<coco_types::Message>>>,
}

impl SessionHandle {
    fn new(session_id: String, cwd: String, model: String) -> Self {
        Self {
            session_id,
            cwd,
            model,
            permission_mode: None,
            thinking_level: None,
            env_overrides: std::collections::HashMap::new(),
            active_turn_cancel: None,
            turn_counter: 0,
            started_at: std::time::Instant::now(),
            stats: SessionStats::default(),
            history: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// Aggregated per-session stats, mirrored from per-turn `SessionResult`
/// notifications emitted by `QueryEngine::run_with_events`.
///
/// Each field accumulates across every `turn/start` call in the session.
/// `session/archive` packages this into a single outbound `SessionResult`.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub total_turns: i32,
    pub total_duration_api_ms: i64,
    pub total_cost_usd: f64,
    pub usage: coco_types::TokenUsage,
    pub model_usage: std::collections::HashMap<String, coco_types::SessionModelUsage>,
    pub permission_denials: Vec<coco_types::PermissionDenialInfo>,
    pub last_result_text: Option<String>,
    pub last_stop_reason: Option<String>,
    pub had_error: bool,
    pub errors: Vec<String>,
    pub num_api_calls: i32,
}

/// Per-request context passed to handlers.
pub struct HandlerContext {
    /// Channel for forwarding CoreEvent notifications to the transport.
    /// Handlers that spawn a QueryEngine pass this as the engine's
    /// `event_tx`. Single-shot handlers (e.g., `initialize`) rarely use
    /// it; long-running handlers (e.g., `turn/start`) emit events here.
    pub notif_tx: mpsc::Sender<CoreEvent>,

    /// Shared server state across requests (session slot).
    pub state: Arc<SdkServerState>,
}

/// Result of dispatching a ClientRequest.
pub enum HandlerResult {
    /// Handler succeeded — carries the response `result` payload.
    Ok(Value),
    /// Handler failed with a JSON-RPC error.
    Err {
        code: i32,
        message: String,
        data: Option<Value>,
    },
    /// Handler is not implemented in the current phase. The dispatcher
    /// converts this to a `JsonRpcError` with `METHOD_NOT_FOUND`.
    NotImplemented(String),
}

impl HandlerResult {
    /// Shorthand for a successful empty response.
    pub fn ok_empty() -> Self {
        Self::Ok(Value::Null)
    }

    /// Build an Ok result from any serializable payload. Handler errors
    /// on serialization failure (rare in practice).
    pub fn ok<T: serde::Serialize>(value: T) -> Self {
        match serde_json::to_value(value) {
            Ok(v) => Self::Ok(v),
            Err(e) => Self::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("result serialization failed: {e}"),
                data: None,
            },
        }
    }
}

/// Route a `ClientRequest` to its handler and return the result.
///
/// Phase 2.C.1: all handlers are stubs returning `NotImplemented`. Phase
/// 2.C.2+ will fill in real implementations. The dispatch structure is
/// already exhaustive so adding a new variant to `ClientRequest` will
/// fail compilation here — enforcing that every method has a handler.
pub async fn dispatch_client_request(req: ClientRequest, ctx: HandlerContext) -> HandlerResult {
    match req {
        // === Session lifecycle ===
        ClientRequest::Initialize(params) => handle_initialize(params, &ctx).await,
        ClientRequest::SessionStart(params) => handle_session_start(*params, &ctx).await,
        ClientRequest::SessionResume(params) => handle_session_resume(params, &ctx).await,
        ClientRequest::SessionList => handle_session_list(&ctx).await,
        ClientRequest::SessionRead(params) => handle_session_read(params, &ctx).await,
        ClientRequest::SessionArchive(params) => handle_session_archive(params, &ctx).await,

        // === Turn control ===
        ClientRequest::TurnStart(params) => handle_turn_start(params, &ctx).await,
        ClientRequest::TurnInterrupt => handle_turn_interrupt(&ctx).await,

        // === Approval + user input + elicitation ===
        ClientRequest::ApprovalResolve(params) => handle_approval_resolve(params, &ctx).await,
        ClientRequest::UserInputResolve(params) => handle_user_input_resolve(params, &ctx).await,
        ClientRequest::ElicitationResolve(params) => handle_elicitation_resolve(params, &ctx).await,

        // === Runtime control ===
        ClientRequest::SetModel(params) => handle_set_model(params, &ctx).await,
        ClientRequest::SetPermissionMode(params) => handle_set_permission_mode(params, &ctx).await,
        ClientRequest::SetThinking(params) => handle_set_thinking(params, &ctx).await,
        ClientRequest::StopTask(params) => handle_stop_task(params, &ctx).await,
        ClientRequest::RewindFiles(params) => handle_rewind_files(params, &ctx).await,
        ClientRequest::UpdateEnv(params) => handle_update_env(params, &ctx).await,

        // `keepAlive` is the simplest handler — respond with empty ok so
        // clients using it as a heartbeat get immediate acknowledgement.
        ClientRequest::KeepAlive => HandlerResult::ok_empty(),

        ClientRequest::CancelRequest(params) => handle_cancel_request(params, &ctx).await,

        // === Config ===
        ClientRequest::ConfigRead => handle_config_read(&ctx).await,
        ClientRequest::ConfigWrite(params) => handle_config_write(params, &ctx).await,

        // === Hook + MCP routing responses ===
        ClientRequest::HookCallbackResponse(params) => {
            handle_hook_callback_response(params, &ctx).await
        }
        ClientRequest::McpRouteMessageResponse(params) => {
            handle_mcp_route_message_response(params, &ctx).await
        }

        // === TS P1 gap additions ===
        ClientRequest::McpStatus => handle_mcp_status(&ctx).await,
        ClientRequest::ContextUsage => handle_context_usage(&ctx).await,
        ClientRequest::McpSetServers(params) => handle_mcp_set_servers(params, &ctx).await,
        ClientRequest::McpReconnect(params) => handle_mcp_reconnect(params, &ctx).await,
        ClientRequest::McpToggle(params) => handle_mcp_toggle(params, &ctx).await,
        ClientRequest::PluginReload => handle_plugin_reload(&ctx).await,
        ClientRequest::ConfigApplyFlags(params) => handle_config_apply_flags(params, &ctx).await,
    }
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// `initialize` — capability negotiation. Synchronous; returns a
/// precomputed `InitializeResult` with protocol version, binary version,
/// and a static model list.
///
/// Phase 2.C.2 returns a minimal set. Phase 2.C.7 will expand it with
/// dynamic tool / command / agent registry lookups once those registries
/// are threaded through `SdkServerState`.
///
/// TS reference: `SDKControlInitializeRequestSchema` +
/// `SDKControlInitializeResponseSchema` in `controlSchemas.ts:57-95`.
async fn handle_initialize(
    _params: coco_types::InitializeParams,
    _ctx: &HandlerContext,
) -> HandlerResult {
    info!("SdkServer: initialize");
    let result = InitializeResult {
        protocol_version: PROTOCOL_VERSION.into(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tools: Vec::new(),
        commands: Vec::new(),
        agents: Vec::new(),
        models: vec![
            SdkModelInfo {
                id: "claude-opus-4-6".into(),
                display_name: "Claude Opus 4.6".into(),
                context_window: Some(200_000),
                max_output_tokens: Some(16_384),
            },
            SdkModelInfo {
                id: "claude-sonnet-4-6".into(),
                display_name: "Claude Sonnet 4.6".into(),
                context_window: Some(200_000),
                max_output_tokens: Some(16_384),
            },
        ],
        pid: Some(std::process::id()),
    };
    HandlerResult::ok(result)
}

/// `session/start` — create a new SDK session.
///
/// Phase 2.C.2 records the session in `SdkServerState.session` and returns
/// a generated `session_id`. The actual QueryEngine is not spawned until
/// Phase 2.C.3 wires `turn/start`.
///
/// TS reference: `print.ts runHeadless()` creates a session at the top of
/// headless mode; coco-rs lets the SDK client explicitly trigger this via
/// `session/start` instead.
async fn handle_session_start(
    params: coco_types::SessionStartParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut session_slot = ctx.state.session.write().await;
    if session_slot.is_some() {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "a session is already active; archive it first or use session/resume".into(),
            data: None,
        };
    }

    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    let cwd = params.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let model = params
        .model
        .clone()
        .unwrap_or_else(|| "claude-opus-4-6".into());

    info!(session_id = %session_id, cwd = %cwd, model = %model, "SdkServer: session/start");

    // Persist to disk if a SessionManager is wired. This makes the
    // session visible to `session/list` and resumable via
    // `session/resume`. Failure to persist is non-fatal — the session
    // still runs in-memory; we log a warning and continue.
    {
        let manager_slot = ctx.state.session_manager.read().await;
        if let Some(manager) = manager_slot.as_ref() {
            let record = coco_session::Session {
                id: session_id.clone(),
                created_at: timestamp_now(),
                updated_at: None,
                model: model.clone(),
                working_dir: std::path::PathBuf::from(&cwd),
                title: None,
                message_count: 0,
                total_tokens: 0,
            };
            if let Err(e) = manager.save(&record) {
                warn!(session_id = %session_id, error = %e, "session/start: failed to persist session to disk");
            }
        }
    }

    *session_slot = Some(SessionHandle::new(session_id.clone(), cwd, model));

    HandlerResult::ok(SessionStartResult { session_id })
}

/// Current timestamp in RFC 3339 format (UTC). Matches
/// `coco_session`'s internal `timestamp_now` without requiring a
/// cross-crate dep on its private helper.
fn timestamp_now() -> String {
    // SystemTime → seconds since epoch → ISO-ish string.
    // For formatting parity with coco_session we use the same approach:
    // the dispatcher doesn't care about exact format as long as it sorts.
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// `turn/start` — begin a single agent turn in the active session.
///
/// Fire-and-forget: the dispatcher delegates to the configured
/// [`TurnRunner`] (spawned on a detached task) and replies immediately
/// with a `turn_id`. Progress flows back via `turn/started`, streaming
/// deltas, and `turn/completed` / `turn/failed` notifications on the
/// shared `notif_tx` channel.
///
/// Errors:
/// - `INVALID_REQUEST` if no session is active.
/// - `INVALID_REQUEST` if a turn is already in flight (one-at-a-time).
///
/// TS reference: `runHeadless()` inside `print.ts` kicks off a single
/// turn per headless invocation; coco-rs lets the SDK client drive the
/// cadence via `turn/start`.
async fn handle_turn_start(params: TurnStartParams, ctx: &HandlerContext) -> HandlerResult {
    // Grab the active session and reserve an active_turn_cancel slot.
    let (session_snapshot, turn_id, cancel_token) = {
        let mut slot = ctx.state.session.write().await;
        let Some(session) = slot.as_mut() else {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: "no active session; call session/start first".into(),
                data: None,
            };
        };
        if session.active_turn_cancel.is_some() {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: "a turn is already running; call turn/interrupt first".into(),
                data: None,
            };
        }
        session.turn_counter = session.turn_counter.saturating_add(1);
        let turn_id = format!("turn-{}-{}", session.session_id, session.turn_counter);
        let cancel_token = CancellationToken::new();
        session.active_turn_cancel = Some(cancel_token.clone());
        (session.clone(), turn_id, cancel_token)
    };

    info!(
        session_id = %session_snapshot.session_id,
        turn_id = %turn_id,
        "SdkServer: turn/start"
    );

    // Event-forwarder bridge: the runner writes to `inner_tx`; the
    // forwarder task reads events, intercepts `SessionResult` to fold
    // per-turn stats into `SessionHandle.stats`, and forwards everything
    // else (sans SessionStarted / SessionResult) to the real `notif_tx`.
    //
    // This decouples the engine's "one SessionResult per run_with_events"
    // assumption from the SDK's "one SessionResult per session" wire
    // contract. See Phase 2.C.7 in event-system-design.md.
    //
    // The forwarder is parameterized by the owner session_id so it can
    // refuse to fold stats into a DIFFERENT session after archive +
    // session/start has replaced the slot (see Fix #F3 in third-round
    // review).
    let (inner_tx, inner_rx) = mpsc::channel::<CoreEvent>(256);
    tokio::spawn(forward_turn_events(
        inner_rx,
        ctx.notif_tx.clone(),
        ctx.state.clone(),
        session_snapshot.session_id.clone(),
    ));

    // Spawn the turn as a detached task so `turn/start` returns the
    // turn_id synchronously.
    let runner = ctx.state.turn_runner.read().await.clone();
    let state = ctx.state.clone();
    let cancel_for_task = cancel_token.clone();
    let session_for_task = session_snapshot.clone();
    let turn_id_for_task = turn_id.clone();
    let owner_session_id = session_snapshot.session_id.clone();
    tokio::spawn(async move {
        let run_result = runner
            .run_turn(params, session_for_task, inner_tx, cancel_for_task)
            .await;
        if let Err(e) = run_result {
            warn!(turn_id = %turn_id_for_task, error = %e, "turn runner failed");
        }
        // Clear the active_turn_cancel slot so the next turn can start.
        //
        // Cross-session guard: only clear if the session in the slot is
        // STILL the session this turn belonged to. If `session/archive`
        // + `session/start` ran while this turn was winding down, the
        // slot now holds a different session — mutating its
        // `active_turn_cancel` would silently corrupt its turn state.
        let mut slot = state.session.write().await;
        if let Some(session) = slot.as_mut()
            && session.session_id == owner_session_id
        {
            session.active_turn_cancel = None;
        }
    });

    HandlerResult::ok(coco_types::TurnStartResult { turn_id })
}

/// Drain per-turn CoreEvents and forward to the outbound notification
/// channel, intercepting session envelope events.
///
/// Specifically:
/// - `SessionResult` events are **not** forwarded. Instead, their stats
///   are folded into `SessionHandle.stats` (only if the current session
///   still matches `owner_session_id`). The aggregated `SessionResult`
///   is emitted once when `session/archive` runs.
/// - `SessionStarted` events are also swallowed (defensive — the current
///   runner doesn't emit them, but if a future runner enables the
///   bootstrap path, we still want exactly one per session from the SDK
///   server side, not one per turn).
/// - All other events pass through unchanged.
///
/// `owner_session_id` is the session this forwarder was created for.
/// If `session/archive` + `session/start` has replaced the active
/// session while this forwarder is still draining, we refuse to fold
/// stats into the unrelated new session (see Fix #F3).
async fn forward_turn_events(
    mut rx: mpsc::Receiver<CoreEvent>,
    tx: mpsc::Sender<CoreEvent>,
    state: Arc<SdkServerState>,
    owner_session_id: String,
) {
    use coco_types::ServerNotification;
    while let Some(event) = rx.recv().await {
        match event {
            CoreEvent::Protocol(ServerNotification::SessionResult(params)) => {
                accumulate_session_result(&state, &owner_session_id, &params).await;
                // Swallow — aggregated result is emitted by session/archive.
            }
            CoreEvent::Protocol(ServerNotification::SessionStarted(_)) => {
                // Swallow: SessionStarted is owned by the SDK server, not the engine.
            }
            other => {
                if tx.send(other).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Fold a per-turn `SessionResult` into the active session's aggregated
/// stats. No-op if:
/// - the session has already been archived, OR
/// - the current session's id doesn't match `owner_session_id` (the
///   turn belongs to an already-archived session whose cleanup is
///   still winding down; we must not contaminate the successor session)
async fn accumulate_session_result(
    state: &SdkServerState,
    owner_session_id: &str,
    params: &coco_types::SessionResultParams,
) {
    let mut slot = state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return;
    };
    if session.session_id != owner_session_id {
        // Cross-session guard: the slot now holds a different session.
        // This turn's stats belong to a dead session — drop them.
        debug!(
            owner = owner_session_id,
            current = %session.session_id,
            "accumulate_session_result: session mismatch, dropping stats"
        );
        return;
    }
    let s = &mut session.stats;
    s.total_turns = s.total_turns.saturating_add(1);
    s.total_duration_api_ms = s
        .total_duration_api_ms
        .saturating_add(params.duration_api_ms);
    s.total_cost_usd += params.total_cost_usd;
    s.usage = s.usage + params.usage;
    for (model, mu) in &params.model_usage {
        let entry = s.model_usage.entry(model.clone()).or_default();
        entry.input_tokens += mu.input_tokens;
        entry.output_tokens += mu.output_tokens;
        entry.cache_read_input_tokens += mu.cache_read_input_tokens;
        entry.cache_creation_input_tokens += mu.cache_creation_input_tokens;
        entry.web_search_requests += mu.web_search_requests;
        entry.cost_usd += mu.cost_usd;
    }
    s.permission_denials
        .extend(params.permission_denials.iter().cloned());
    if params.result.is_some() {
        s.last_result_text = params.result.clone();
    }
    s.last_stop_reason = Some(params.stop_reason.clone());
    if params.is_error {
        s.had_error = true;
        s.errors.extend(params.errors.iter().cloned());
    }
    if let Some(n) = params.num_api_calls {
        s.num_api_calls = s.num_api_calls.saturating_add(n);
    }
}

/// `turn/interrupt` — cancel the currently-running turn (if any).
///
/// Cancellation is cooperative: the runner's task is notified via the
/// `CancellationToken` it received from `turn/start`. The runner is
/// expected to observe `cancel.is_cancelled()` at tool boundaries and
/// emit a `turn/failed` notification before exiting.
///
/// TS reference: `SDKControlInterruptRequestSchema` (controlSchemas.ts).
async fn handle_turn_interrupt(ctx: &HandlerContext) -> HandlerResult {
    let slot = ctx.state.session.read().await;
    let Some(session) = slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    match &session.active_turn_cancel {
        Some(token) => {
            info!(session_id = %session.session_id, "SdkServer: turn/interrupt");
            token.cancel();
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no turn in flight to interrupt".into(),
            data: None,
        },
    }
}

/// `approval/resolve` — resolve a pending `approval/askForApproval`
/// ServerRequest with the client's decision.
///
/// The dispatcher holds a map of pending approvals keyed by `request_id`
/// (see [`SdkServerState::pending_approvals`]). When the agent's tool
/// executor hits a gate that needs SDK approval, it registers a oneshot
/// via [`SdkServerState::register_approval`], sends an `AskForApproval`
/// ServerRequest on the wire, and awaits the receiver. This handler
/// completes the round trip by looking up the sender and delivering the
/// client-supplied `ApprovalResolveParams`.
///
/// Errors:
/// - `INVALID_REQUEST` if `request_id` does not match any pending approval.
///   This usually means the client replied twice or is responding to a
///   stale/cancelled request.
///
/// TS reference: `controlSchemas.ts` `SDKControlPermissionRequestSchema`.
async fn handle_approval_resolve(
    params: ApprovalResolveParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let request_id = params.request_id.clone();
    let sender = {
        let mut map = ctx.state.pending_approvals.lock().await;
        map.remove(&request_id)
    };
    match sender {
        Some(tx) => {
            info!(request_id = %request_id, decision = ?params.decision, "SdkServer: approval/resolve");
            // If the agent-side awaiter has been dropped (e.g. the turn
            // was cancelled mid-approval) the send returns Err. We log
            // and still acknowledge to the client so it doesn't hang.
            if tx.send(params).is_err() {
                warn!(
                    request_id = %request_id,
                    "approval/resolve: agent receiver dropped before resolution"
                );
            }
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no pending approval with request_id {request_id}"),
            data: None,
        },
    }
}

/// `elicitation/resolve` — resolve a pending MCP elicitation request
/// with the user's form input (or rejection).
///
/// Mirror of [`handle_approval_resolve`] for the elicitation flow —
/// an MCP server sent a `ServerRequest::RequestElicitation` asking
/// for structured input, the agent registered a oneshot via
/// [`SdkServerState::register_elicitation`], and this handler wakes
/// the waiting MCP client with the populated form values (or a
/// rejection if `approved=false`).
///
/// Errors:
/// - `INVALID_REQUEST` if `request_id` doesn't match any pending
///   elicitation. Typical causes: duplicate resolve, stale
///   request after a turn cancellation, protocol confusion.
async fn handle_elicitation_resolve(
    params: ElicitationResolveParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let request_id = params.request_id.clone();
    let sender = {
        let mut map = ctx.state.pending_elicitations.lock().await;
        map.remove(&request_id)
    };
    match sender {
        Some(tx) => {
            info!(
                request_id = %request_id,
                mcp_server = %params.mcp_server_name,
                approved = params.approved,
                "SdkServer: elicitation/resolve"
            );
            if tx.send(params).is_err() {
                warn!(
                    request_id = %request_id,
                    "elicitation/resolve: agent receiver dropped before delivery"
                );
            }
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no pending elicitation with request_id {request_id}"),
            data: None,
        },
    }
}

/// `input/resolveUserInput` — resolve a pending `input/requestUserInput`
/// ServerRequest with the user's answer.
///
/// Mirror of [`handle_approval_resolve`] for the `RequestUserInput` flow
/// (e.g. free-form questions or multiple-choice prompts surfaced via
/// `AskUserQuestion`-style tools).
async fn handle_user_input_resolve(
    params: UserInputResolveParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let request_id = params.request_id.clone();
    let sender = {
        let mut map = ctx.state.pending_user_input.lock().await;
        map.remove(&request_id)
    };
    match sender {
        Some(tx) => {
            info!(request_id = %request_id, "SdkServer: input/resolveUserInput");
            if tx.send(params).is_err() {
                warn!(
                    request_id = %request_id,
                    "input/resolveUserInput: agent receiver dropped before resolution"
                );
            }
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no pending user input with request_id {request_id}"),
            data: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Phase 2.C.6: session/archive + runtime control handlers
// ---------------------------------------------------------------------------

/// `session/archive` — drop the active session slot.
///
/// Emits the aggregated `SessionResult` (built from the session's
/// accumulated stats) as a final notification before clearing the slot.
/// This gives SDK clients exactly one `SessionResult` per session,
/// regardless of how many `turn/start` calls happened inside it.
///
/// **Ordering note**: The `SessionResult` notification goes through
/// `ctx.notif_tx` (drained by the dispatcher's notification forwarder
/// task) while the archive response goes directly through the
/// transport from the handler. These two paths are not synchronized,
/// so the response may reach the wire *before* the notification. SDK
/// clients that need strict ordering should drain inbound messages
/// until both arrive, matching on type.
///
/// **Archive-during-running-turn**: If a turn is in flight when
/// `session/archive` is called, the aggregate is built from whatever
/// stats have been accumulated so far (the in-flight turn's stats are
/// NOT included — it's cancelled after the aggregate is built). This
/// matches TS headless semantics where archive discards in-progress
/// work.
///
/// Errors:
/// - `INVALID_REQUEST` if no session is active
/// - `INVALID_REQUEST` if the `session_id` param doesn't match the
///   currently-active session (prevents clients from archiving someone
///   else's session by mistake)
async fn handle_session_archive(
    params: coco_types::SessionArchiveParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Hold the write lock for the entire archive operation:
    // (1) validate session_id, (2) build the aggregate from a
    // consistent snapshot, (3) clear the slot. This closes the
    // TOCTOU window that an earlier read/write-lock split opened —
    // a concurrent `forward_turn_events` forwarder cannot slip a
    // `SessionResult` into stats between the aggregate build and
    // the clear, because it contends for the same write lock and we
    // hold it end-to-end here.
    //
    // Cancellation of an in-flight turn and emission of the
    // aggregated notification both happen AFTER the lock is released:
    // cancellation is idempotent and the notification send doesn't
    // need the session lock.
    let (result_params, token_to_cancel) = {
        let mut slot = ctx.state.session.write().await;
        let Some(session) = slot.as_ref() else {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: "no active session to archive".into(),
                data: None,
            };
        };
        if session.session_id != params.session_id {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: format!(
                    "session_id mismatch: active is {}, archive requested for {}",
                    session.session_id, params.session_id
                ),
                data: None,
            };
        }
        info!(session_id = %params.session_id, "SdkServer: session/archive");
        let result = build_aggregated_session_result(session);
        let token = session.active_turn_cancel.clone();
        // Clear the slot under the write lock so any racing forwarder
        // that later acquires the lock sees `None` and no-ops.
        *slot = None;
        (result, token)
    };

    // Cancel any running turn. Outside the lock because:
    //   (a) `CancellationToken::cancel` is cheap and non-blocking
    //   (b) the turn task's subsequent cleanup (writing to session
    //       slot to clear `active_turn_cancel`) also takes the write
    //       lock — holding it here would deadlock
    if let Some(token) = token_to_cancel {
        token.cancel();
    }

    // Delete the persisted session record if a SessionManager is wired.
    // Non-fatal — log and continue if disk delete fails.
    {
        let manager_slot = ctx.state.session_manager.read().await;
        if let Some(manager) = manager_slot.as_ref()
            && let Err(e) = manager.delete(&params.session_id)
        {
            warn!(
                session_id = %params.session_id,
                error = %e,
                "session/archive: failed to delete persisted session record"
            );
        }
    }

    // Emit the aggregated SessionResult on the outbound notification
    // channel. Ignore a send error (transport may have shut down)
    // since the state is already cleared.
    let result_event = CoreEvent::Protocol(coco_types::ServerNotification::SessionResult(
        Box::new(result_params),
    ));
    let _ = ctx.notif_tx.send(result_event).await;

    HandlerResult::ok_empty()
}

/// Build a final `SessionResultParams` from an active session's
/// accumulated stats. Used by `session/archive` to synthesize the
/// once-per-session aggregate the SDK client expects.
fn build_aggregated_session_result(session: &SessionHandle) -> coco_types::SessionResultParams {
    let stats = &session.stats;
    coco_types::SessionResultParams {
        session_id: session.session_id.clone(),
        total_turns: stats.total_turns,
        duration_ms: session.started_at.elapsed().as_millis() as i64,
        duration_api_ms: stats.total_duration_api_ms,
        is_error: stats.had_error,
        stop_reason: stats
            .last_stop_reason
            .clone()
            .unwrap_or_else(|| "archived".into()),
        total_cost_usd: stats.total_cost_usd,
        usage: stats.usage,
        model_usage: stats.model_usage.clone(),
        permission_denials: stats.permission_denials.clone(),
        result: stats.last_result_text.clone(),
        errors: stats.errors.clone(),
        structured_output: None,
        fast_mode_state: None,
        num_api_calls: if stats.num_api_calls > 0 {
            Some(stats.num_api_calls)
        } else {
            None
        },
    }
}

/// `control/setModel` — mutate the active session's model.
///
/// The updated model takes effect on the *next* `turn/start`. In-flight
/// turns continue running against the previous model (they'd need
/// restarting to swap models mid-call).
///
/// Passing `None` means "revert to the default model", which we
/// interpret as `claude-opus-4-6` (the bootstrap default from
/// `handle_session_start`).
async fn handle_set_model(
    params: coco_types::SetModelParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    let new_model = params
        .model
        .clone()
        .unwrap_or_else(|| "claude-opus-4-6".into());
    info!(
        session_id = %session.session_id,
        old_model = %session.model,
        new_model = %new_model,
        "SdkServer: control/setModel"
    );
    session.model = new_model;
    HandlerResult::ok_empty()
}

/// `control/setPermissionMode` — mutate the session's permission mode.
///
/// Stored on the [`SessionHandle`]; the production [`TurnRunner`]
/// reads it when constructing per-turn `QueryEngineConfig` (falling
/// through to the turn-scoped override from `TurnStartParams` if set).
async fn handle_set_permission_mode(
    params: coco_types::SetPermissionModeParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    info!(
        session_id = %session.session_id,
        mode = ?params.mode,
        "SdkServer: control/setPermissionMode"
    );
    session.permission_mode = Some(params.mode);
    HandlerResult::ok_empty()
}

/// `control/setThinking` — mutate the session's thinking level.
///
/// `thinking_level = None` clears the override so turns fall back to
/// the engine's default (matches TS `max_thinking_tokens: null`).
async fn handle_set_thinking(
    params: coco_types::SetThinkingParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    info!(
        session_id = %session.session_id,
        level = ?params.thinking_level,
        "SdkServer: control/setThinking"
    );
    session.thinking_level = params.thinking_level;
    HandlerResult::ok_empty()
}

/// `control/stopTask` — cooperative cancellation of a specific task.
///
/// Coco-rs's in-process background task registry isn't threaded through
/// the SDK server yet, so for Phase 2.C.6 this is structurally equivalent
/// to `turn/interrupt`: we cancel any in-flight turn so the runner
/// unwinds. The `task_id` is logged for later correlation once the
/// task manager is wired through `SdkServerState`.
async fn handle_stop_task(
    params: coco_types::StopTaskParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let slot = ctx.state.session.read().await;
    let Some(session) = slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    match &session.active_turn_cancel {
        Some(token) => {
            info!(
                session_id = %session.session_id,
                task_id = %params.task_id,
                "SdkServer: control/stopTask (cancels active turn)"
            );
            token.cancel();
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no task in flight matching task_id {}", params.task_id),
            data: None,
        },
    }
}

/// `control/updateEnv` — merge environment variable updates into the
/// session's override map.
///
/// Passing an empty string for a value is interpreted as "unset" and
/// removes the key from the override map. The resulting map is passed
/// to tool invocations when the shell executor is wired to read it.
async fn handle_update_env(
    params: coco_types::UpdateEnvParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    let mut applied = 0;
    let mut cleared = 0;
    for (k, v) in params.env {
        if v.is_empty() {
            if session.env_overrides.remove(&k).is_some() {
                cleared += 1;
            }
        } else {
            session.env_overrides.insert(k, v);
            applied += 1;
        }
    }
    info!(
        session_id = %session.session_id,
        applied,
        cleared,
        total = session.env_overrides.len(),
        "SdkServer: control/updateEnv"
    );
    HandlerResult::ok_empty()
}

/// `control/cancelRequest` — cancel a previously-issued ServerRequest.
///
/// The SDK client uses this to abort a `ServerRequest::AskForApproval`
/// (or similar) that it no longer wants to resolve, e.g. if the user
/// closed the approval UI before answering.
///
/// We drop the pending oneshot sender so the agent-side receiver gets
/// an `Err(RecvError)` and the tool executor can treat it as "denied".
/// If the `request_id` isn't in any pending map, we still return ok so
/// the client doesn't treat a race (server already resolved + cleaned
/// up) as a protocol error.
async fn handle_cancel_request(
    params: coco_types::CancelRequestParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let request_id = params.request_id;
    let reason = params.reason.as_deref().unwrap_or("(no reason given)");
    // Try the approval map first, then the user-input map.
    let removed_approval = ctx
        .state
        .pending_approvals
        .lock()
        .await
        .remove(&request_id)
        .is_some();
    let removed_user_input = if removed_approval {
        false
    } else {
        ctx.state
            .pending_user_input
            .lock()
            .await
            .remove(&request_id)
            .is_some()
    };
    if removed_approval || removed_user_input {
        info!(
            request_id = %request_id,
            reason = %reason,
            kind = if removed_approval { "approval" } else { "user_input" },
            "SdkServer: control/cancelRequest"
        );
    } else {
        info!(
            request_id = %request_id,
            reason = %reason,
            "SdkServer: control/cancelRequest — no pending request matched (already resolved?)"
        );
    }
    HandlerResult::ok_empty()
}

// ---------------------------------------------------------------------------
// Phase 2.C.11: session/list + session/read + session/resume
// ---------------------------------------------------------------------------

/// Convert a `coco_session::Session` record to the wire-format
/// summary used by list/read/resume results.
fn session_to_summary(s: &coco_session::Session) -> coco_types::SdkSessionSummary {
    coco_types::SdkSessionSummary {
        session_id: s.id.clone(),
        model: s.model.clone(),
        cwd: s.working_dir.to_string_lossy().into_owned(),
        created_at: s.created_at.clone(),
        updated_at: s.updated_at.clone(),
        title: s.title.clone(),
        message_count: s.message_count,
        total_tokens: s.total_tokens,
    }
}

/// `session/list` — enumerate persisted sessions, newest first.
///
/// Delegates to `SessionManager::list()`. Returns an empty list if no
/// manager is wired (session persistence disabled).
///
/// Errors:
/// - `INTERNAL_ERROR` if `SessionManager::list()` fails (e.g. filesystem error)
async fn handle_session_list(ctx: &HandlerContext) -> HandlerResult {
    let manager = ctx.state.session_manager.read().await;
    let Some(manager) = manager.as_ref() else {
        info!("SdkServer: session/list (no session manager installed, returning empty)");
        return HandlerResult::ok(coco_types::SessionListResult::default());
    };
    match manager.list() {
        Ok(sessions) => {
            let summaries = sessions.iter().map(session_to_summary).collect::<Vec<_>>();
            info!(count = summaries.len(), "SdkServer: session/list");
            HandlerResult::ok(coco_types::SessionListResult {
                sessions: summaries,
            })
        }
        Err(e) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("session/list failed: {e}"),
            data: None,
        },
    }
}

/// `session/read` — load a single persisted session's metadata.
///
/// Phase 2.C.11 returns the summary only; message history retrieval
/// via the JSONL transcript is reserved for a follow-up.
///
/// Errors:
/// - `INVALID_REQUEST` if no session manager is wired
/// - `INVALID_REQUEST` if the session_id is not found on disk
async fn handle_session_read(
    params: coco_types::SessionReadParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager = ctx.state.session_manager.read().await;
    let Some(manager) = manager.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "session persistence is not enabled on this server".into(),
            data: None,
        };
    };
    match manager.load(&params.session_id) {
        Ok(session) => {
            info!(session_id = %params.session_id, "SdkServer: session/read");
            HandlerResult::ok(coco_types::SessionReadResult {
                session: session_to_summary(&session),
                messages: Vec::new(),
                next_cursor: None,
                has_more: false,
            })
        }
        Err(e) => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("session/read: {e}"),
            data: None,
        },
    }
}

/// `session/resume` — load a persisted session from disk and install
/// it as the active session.
///
/// Replaces the current session slot (if any) with a fresh
/// `SessionHandle` built from the persisted metadata. Any in-flight
/// turn on the previous session is cancelled first to prevent
/// orphaned state.
///
/// Note: Phase 2.C.11 restores session metadata (id, model, cwd) but
/// does NOT reload the message history from the JSONL transcript. The
/// resumed session starts with an empty history. A follow-up will
/// thread the transcript reader in.
///
/// Errors:
/// - `INVALID_REQUEST` if no session manager is wired
/// - `INVALID_REQUEST` if the session_id is not found on disk
/// - `INTERNAL_ERROR` if the session manager's resume operation fails
async fn handle_session_resume(
    params: coco_types::SessionResumeParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_slot = ctx.state.session_manager.read().await;
    let Some(manager) = manager_slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "session persistence is not enabled on this server".into(),
            data: None,
        };
    };
    let session = match manager.resume(&params.session_id) {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: format!("session/resume: {e}"),
                data: None,
            };
        }
    };
    // Release the manager read lock before acquiring the session write lock
    // to avoid potential lock-ordering complications in future refactors.
    drop(manager_slot);

    // Install as the active session. If a session is already active,
    // cancel any in-flight turn and replace it — `session/resume`
    // implicitly archives the prior session.
    let mut slot = ctx.state.session.write().await;
    if let Some(prior) = slot.as_ref()
        && let Some(token) = &prior.active_turn_cancel
    {
        warn!(
            prior_session = %prior.session_id,
            new_session = %session.id,
            "SdkServer: session/resume replaces active session; cancelling in-flight turn"
        );
        token.cancel();
    }
    *slot = Some(SessionHandle::new(
        session.id.clone(),
        session.working_dir.to_string_lossy().into_owned(),
        session.model.clone(),
    ));

    info!(session_id = %session.id, "SdkServer: session/resume");
    HandlerResult::ok(coco_types::SessionResumeResult {
        session: session_to_summary(&session),
    })
}

// ---------------------------------------------------------------------------
// Phase 2.C.12: config/read + config/value/write
// ---------------------------------------------------------------------------

/// `config/read` — return the merged effective configuration plus a
/// per-source breakdown keyed by source name.
///
/// Delegates to [`coco_config::settings::load_settings`] with the
/// session's cwd (if a session is active) or the CLI's cwd as the
/// project root. Returns the JSON-serialized merged view and a
/// per-source map suitable for clients that want to display or
/// override specific layers.
///
/// TS reference: `SDKControlGetSettingsRequestSchema` /
/// `SDKControlGetSettingsResponseSchema` in `controlSchemas.ts`.
async fn handle_config_read(ctx: &HandlerContext) -> HandlerResult {
    // Resolve cwd — prefer active session's cwd, fall back to
    // process cwd. Project/local settings live under cwd, so this
    // matters for clients that have multiple repos open.
    let cwd = {
        let slot = ctx.state.session.read().await;
        slot.as_ref()
            .map(|s| std::path::PathBuf::from(&s.cwd))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    };

    let loaded = match coco_config::settings::load_settings(&cwd, None) {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("config/read: failed to load settings: {e}"),
                data: None,
            };
        }
    };

    // Serialize the merged settings as JSON for the wire.
    let merged_json = match serde_json::to_value(&loaded.merged) {
        Ok(v) => v,
        Err(e) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("config/read: failed to serialize settings: {e}"),
                data: None,
            };
        }
    };

    // Flatten the per-source map to string keys for the wire format.
    let mut sources: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for (source, value) in &loaded.per_source {
        sources.insert(source.to_string(), value.clone());
    }

    info!(sources = sources.len(), "SdkServer: config/read");
    HandlerResult::ok(coco_types::ConfigReadResult {
        config: merged_json,
        sources,
    })
}

/// `config/value/write` — persist a single setting to the user,
/// project, or local settings file.
///
/// Supports dotted key paths like `"permissions.default_mode"` which
/// are navigated as nested JSON objects (intermediate objects are
/// created as needed).
///
/// Scope defaults to `"user"` (`~/.coco/settings.json`) if not
/// specified. Valid scopes: `"user"`, `"project"`, `"local"`.
///
/// Errors:
/// - `INVALID_PARAMS` if scope is not one of user/project/local
/// - `INTERNAL_ERROR` on filesystem or JSON serialization failure
///
/// TS reference: `SDKControlWriteSettingValueRequestSchema` in
/// `controlSchemas.ts`.
async fn handle_config_write(
    params: coco_types::ConfigWriteParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let scope = params.scope.as_deref().unwrap_or("user");
    let cwd = {
        let slot = ctx.state.session.read().await;
        slot.as_ref()
            .map(|s| std::path::PathBuf::from(&s.cwd))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    };

    let target_path = match scope {
        "user" => coco_config::global_config::user_settings_path(),
        "project" => coco_config::global_config::project_settings_path(&cwd),
        "local" => coco_config::global_config::local_settings_path(&cwd),
        other => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_PARAMS,
                message: format!(
                    "config/value/write: invalid scope {other:?}; expected user|project|local"
                ),
                data: None,
            };
        }
    };

    // Load the existing settings file (empty object if missing).
    let mut doc: serde_json::Value = if target_path.exists() {
        match std::fs::read_to_string(&target_path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(v) => v,
                Err(e) => {
                    return HandlerResult::Err {
                        code: coco_types::error_codes::INTERNAL_ERROR,
                        message: format!(
                            "config/value/write: existing file at {} is not valid JSON: {e}",
                            target_path.display()
                        ),
                        data: None,
                    };
                }
            },
            Err(e) => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INTERNAL_ERROR,
                    message: format!(
                        "config/value/write: failed to read {}: {e}",
                        target_path.display()
                    ),
                    data: None,
                };
            }
        }
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    // Navigate dotted path, creating intermediate objects as needed.
    if let Err(e) = set_nested_json_key(&mut doc, &params.key, params.value.clone()) {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_PARAMS,
            message: format!("config/value/write: {e}"),
            data: None,
        };
    }

    // Ensure parent directory exists before writing.
    if let Some(parent) = target_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("config/value/write: failed to create parent dir: {e}"),
            data: None,
        };
    }

    let serialized = match serde_json::to_string_pretty(&doc) {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("config/value/write: failed to serialize: {e}"),
                data: None,
            };
        }
    };
    if let Err(e) = std::fs::write(&target_path, serialized) {
        return HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!(
                "config/value/write: failed to write {}: {e}",
                target_path.display()
            ),
            data: None,
        };
    }

    info!(
        key = %params.key,
        scope = %scope,
        path = %target_path.display(),
        "SdkServer: config/value/write"
    );
    HandlerResult::ok_empty()
}

/// Set a dotted-path key on a JSON object, creating intermediate
/// objects as needed. Used by `config/value/write` so clients can
/// target nested settings like `"permissions.default_mode"`.
///
/// Errors if an intermediate path segment exists but is not an object
/// (e.g. `a.b.c` where `a.b` is a string).
fn set_nested_json_key(
    doc: &mut serde_json::Value,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    if !doc.is_object() {
        *doc = serde_json::Value::Object(serde_json::Map::new());
    }
    let segments: Vec<&str> = key.split('.').collect();
    if segments.is_empty() || segments.iter().any(|s| s.is_empty()) {
        return Err(format!("invalid key path {key:?}"));
    }
    let mut cursor = doc;
    for (i, segment) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let obj = cursor
            .as_object_mut()
            .ok_or_else(|| format!("path segment {segment:?} is not an object"))?;
        if is_last {
            obj.insert((*segment).to_string(), value);
            return Ok(());
        }
        // Descend, creating an empty object if the intermediate is
        // missing OR not an object.
        let entry = obj
            .entry((*segment).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = serde_json::Value::Object(serde_json::Map::new());
        }
        cursor = entry;
    }
    unreachable!("segments vec is non-empty, loop returns on last iteration")
}

// ---------------------------------------------------------------------------
// Phase 2.C.13: small observability + response handlers
// ---------------------------------------------------------------------------

/// `hook/callbackResponse` — client→server reply to a prior
/// `hook/callback` ServerRequest. Delivers the hook output via the
/// oneshot registered by `register_hook_callback`.
///
/// The server's hook orchestration (future Phase 2.C.18) registers
/// the oneshot before sending `hook/callback`; this handler wakes
/// the awaiting tool loop.
///
/// Errors:
/// - `INVALID_REQUEST` if no pending callback matches `callback_id`
async fn handle_hook_callback_response(
    params: coco_types::ClientHookCallbackResponseParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let callback_id = params.callback_id.clone();
    let sender = {
        let mut map = ctx.state.pending_hook_callbacks.lock().await;
        map.remove(&callback_id)
    };
    match sender {
        Some(tx) => {
            info!(callback_id = %callback_id, "SdkServer: hook/callbackResponse");
            if tx.send(params).is_err() {
                warn!(
                    callback_id = %callback_id,
                    "hook/callbackResponse: agent receiver dropped before delivery"
                );
            }
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no pending hook callback with callback_id {callback_id}"),
            data: None,
        },
    }
}

/// `mcp/routeMessageResponse` — client→server reply to a prior
/// `mcp/routeMessage` ServerRequest. Delivers the forwarded
/// JSON-RPC response via the oneshot registered by
/// `register_mcp_route`.
///
/// Errors:
/// - `INVALID_REQUEST` if no pending route matches `request_id`
async fn handle_mcp_route_message_response(
    params: coco_types::McpRouteMessageResponseParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let request_id = params.request_id.clone();
    let sender = {
        let mut map = ctx.state.pending_mcp_routes.lock().await;
        map.remove(&request_id)
    };
    match sender {
        Some(tx) => {
            info!(request_id = %request_id, "SdkServer: mcp/routeMessageResponse");
            if tx.send(params).is_err() {
                warn!(
                    request_id = %request_id,
                    "mcp/routeMessageResponse: agent receiver dropped before delivery"
                );
            }
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no pending mcp route with request_id {request_id}"),
            data: None,
        },
    }
}

/// `mcp/status` — report MCP server connection status.
///
/// If an `McpConnectionManager` is wired (Phase 2.C.14c onward), this
/// returns the actual connection state for every registered server.
/// Otherwise returns an empty list (persistence disabled).
///
/// TS reference: `SDKControlMcpStatusResponseSchema`
/// (controlSchemas.ts:165-173).
async fn handle_mcp_status(ctx: &HandlerContext) -> HandlerResult {
    let manager_slot = ctx.state.mcp_manager.read().await;
    let Some(manager) = manager_slot.as_ref() else {
        info!("SdkServer: mcp/status (no MCP manager wired, returning empty)");
        return HandlerResult::ok(coco_types::McpStatusResult {
            mcp_servers: Vec::new(),
        });
    };
    let manager = manager.lock().await;
    let names = manager.registered_server_names();
    let mut statuses: Vec<coco_types::McpServerStatus> = Vec::new();
    for name in &names {
        let state = manager.get_state(name).await;
        let (status, error, tool_count) = match state {
            Some(coco_mcp::McpConnectionState::Connected(server)) => {
                let count = server.tools.len() as i32;
                ("connected", None, count)
            }
            Some(coco_mcp::McpConnectionState::Pending { .. }) => ("connecting", None, 0),
            Some(coco_mcp::McpConnectionState::Failed { error }) => ("failed", Some(error), 0),
            Some(coco_mcp::McpConnectionState::NeedsAuth { .. }) => ("needs_auth", None, 0),
            Some(coco_mcp::McpConnectionState::Disabled) => ("disabled", None, 0),
            None => ("disconnected", None, 0),
        };
        statuses.push(coco_types::McpServerStatus {
            name: name.clone(),
            status: status.into(),
            tool_count,
            error,
        });
    }
    info!(server_count = statuses.len(), "SdkServer: mcp/status");
    HandlerResult::ok(coco_types::McpStatusResult {
        mcp_servers: statuses,
    })
}

/// `context/usage` — return the active session's token usage
/// breakdown.
///
/// Phase 2.C.13 derives this from `SessionHandle.stats` which is
/// folded from per-turn `SessionResult` events. This is a coarse
/// total — the rich per-category breakdown from TS (system prompt,
/// tools, history, etc.) is not yet computed; `categories` is
/// empty. A follow-up could wire this via engine-level accounting.
///
/// Errors:
/// - `INVALID_REQUEST` if no session is active
async fn handle_context_usage(ctx: &HandlerContext) -> HandlerResult {
    let slot = ctx.state.session.read().await;
    let Some(session) = slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session; call session/start first".into(),
            data: None,
        };
    };
    let stats = &session.stats;
    // Default context window used by QueryEngineRunner. A future
    // refactor can make this dynamic per-model.
    let max_tokens: i64 = 200_000;
    let total = stats.usage.input_tokens + stats.usage.output_tokens;
    let percentage = if max_tokens > 0 {
        (total as f64 / max_tokens as f64) * 100.0
    } else {
        0.0
    };
    HandlerResult::ok(coco_types::ContextUsageResult {
        total_tokens: total,
        max_tokens,
        raw_max_tokens: max_tokens,
        percentage,
        model: session.model.clone(),
        categories: Vec::new(),
        is_auto_compact_enabled: true,
        auto_compact_threshold: None,
        message_breakdown: None,
    })
}

/// `plugin/reload` — hot-reload plugins.
///
/// Phase 2.C.13 returns an empty result since the SDK server does
/// not yet expose a plugin manager. Acknowledges the client's
/// request so heartbeat-style usage works.
///
/// TS reference: `SDKControlReloadPluginsResponseSchema`.
async fn handle_plugin_reload(_ctx: &HandlerContext) -> HandlerResult {
    info!("SdkServer: plugin/reload (no plugin manager wired, returning empty)");
    HandlerResult::ok(coco_types::PluginReloadResult {
        plugins: Vec::new(),
        commands: Vec::new(),
        agents: Vec::new(),
        error_count: 0,
    })
}

/// `config/applyFlags` — apply runtime feature-flag settings.
///
/// Phase 2.C.13 logs the flags and acks. A follow-up could merge
/// them into a runtime overrides map on `SdkServerState` so other
/// handlers see the effective values.
///
/// TS reference: `SDKControlApplyFlagSettingsRequestSchema`.
async fn handle_config_apply_flags(
    params: coco_types::ConfigApplyFlagsParams,
    _ctx: &HandlerContext,
) -> HandlerResult {
    info!(
        count = params.settings.len(),
        "SdkServer: config/applyFlags (logged; no runtime override map yet)"
    );
    HandlerResult::ok_empty()
}

// ---------------------------------------------------------------------------
// Phase 2.C.14b: control/rewindFiles
// ---------------------------------------------------------------------------

/// `control/rewindFiles` — restore tracked files to a snapshot keyed
/// by `user_message_id`.
///
/// In `dry_run=true` mode, returns a preview (file list + diff stats)
/// without modifying disk. In `dry_run=false` mode, performs the
/// actual restore by writing the backed-up file contents back to
/// their original paths.
///
/// Requires:
/// - An active session (for the session_id used to key file backups)
/// - A `FileHistoryState` installed via `SdkServer::with_file_history()`
///
/// Errors:
/// - `INVALID_REQUEST` if no active session
/// - `INVALID_REQUEST` if file history is not enabled on this server
/// - `INVALID_REQUEST` if `user_message_id` doesn't match any snapshot
/// - `INTERNAL_ERROR` if the rewind / diff operation fails (filesystem)
///
/// TS reference: `SDKControlRewindFilesRequestSchema` (controlSchemas.ts).
async fn handle_rewind_files(
    params: coco_types::RewindFilesParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Resolve the active session_id.
    let session_id = {
        let slot = ctx.state.session.read().await;
        match slot.as_ref() {
            Some(s) => s.session_id.clone(),
            None => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INVALID_REQUEST,
                    message: "no active session; call session/start first".into(),
                    data: None,
                };
            }
        }
    };

    // Resolve the file history + config home.
    let history_arc = {
        let slot = ctx.state.file_history.read().await;
        match slot.as_ref() {
            Some(h) => h.clone(),
            None => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INVALID_REQUEST,
                    message: "control/rewindFiles: file history not enabled on this server".into(),
                    data: None,
                };
            }
        }
    };
    let config_home = {
        let slot = ctx.state.file_history_config_home.read().await;
        match slot.as_ref() {
            Some(p) => p.clone(),
            None => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INVALID_REQUEST,
                    message: "control/rewindFiles: file history config home not set".into(),
                    data: None,
                };
            }
        }
    };

    // Verify the snapshot exists before attempting the operation —
    // gives a clearer error than "rewind failed: not found".
    {
        let history = history_arc.read().await;
        if !history.can_restore(&params.user_message_id) {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: format!(
                    "control/rewindFiles: no snapshot for user_message_id {}",
                    params.user_message_id
                ),
                data: None,
            };
        }
    }

    if params.dry_run {
        // Preview path — get diff stats without touching disk.
        let history = history_arc.read().await;
        let stats = match history
            .get_diff_stats(&params.user_message_id, &config_home, &session_id)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INTERNAL_ERROR,
                    message: format!("control/rewindFiles dry_run: {e}"),
                    data: None,
                };
            }
        };
        info!(
            user_message_id = %params.user_message_id,
            files = stats.files_changed.len(),
            "SdkServer: control/rewindFiles (dry_run)"
        );
        HandlerResult::ok(coco_types::RewindFilesResult {
            files_changed: stats
                .files_changed
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            insertions: stats.insertions,
            deletions: stats.deletions,
            dry_run: true,
        })
    } else {
        // Apply path — get diff stats first for the response payload,
        // then perform the rewind.
        let stats = {
            let history = history_arc.read().await;
            match history
                .get_diff_stats(&params.user_message_id, &config_home, &session_id)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    return HandlerResult::Err {
                        code: coco_types::error_codes::INTERNAL_ERROR,
                        message: format!("control/rewindFiles preview: {e}"),
                        data: None,
                    };
                }
            }
        };
        let history = history_arc.read().await;
        let restored = match history
            .rewind(&params.user_message_id, &config_home, &session_id)
            .await
        {
            Ok(paths) => paths,
            Err(e) => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INTERNAL_ERROR,
                    message: format!("control/rewindFiles: {e}"),
                    data: None,
                };
            }
        };
        info!(
            user_message_id = %params.user_message_id,
            files = restored.len(),
            "SdkServer: control/rewindFiles (applied)"
        );
        HandlerResult::ok(coco_types::RewindFilesResult {
            files_changed: restored
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            insertions: stats.insertions,
            deletions: stats.deletions,
            dry_run: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Phase 2.C.14c: MCP lifecycle handlers
// ---------------------------------------------------------------------------

/// No-op `SendElicitation` callback used when the SDK server's MCP
/// lifecycle handlers trigger a connect that surfaces an elicitation
/// from the upstream server.
///
/// In the SDK design, elicitations from MCP servers should propagate
/// to the SDK client via a `ServerRequest::RequestElicitation` and
/// `elicitation/resolve` round-trip. Wiring that bridge is a future
/// follow-up — until then, this stub immediately rejects any
/// elicitation so connect either succeeds (no auth needed) or errors
/// out (auth required) without blocking forever.
fn no_op_send_elicitation() -> coco_mcp::SendElicitation {
    use std::future::Future;
    use std::pin::Pin;
    Box::new(
        |_request_id, _elicitation| -> Pin<
            Box<dyn Future<Output = anyhow::Result<coco_mcp::ElicitationResponse>> + Send>,
        > {
            Box::pin(async move {
                Err(anyhow::anyhow!(
                    "elicitation rejected: SDK server does not yet bridge elicitations to clients"
                ))
            })
        },
    )
}

/// Helper: borrow the wired MCP manager or return INVALID_REQUEST.
async fn require_mcp_manager(
    ctx: &HandlerContext,
) -> Result<Arc<Mutex<coco_mcp::McpConnectionManager>>, HandlerResult> {
    let slot = ctx.state.mcp_manager.read().await;
    match slot.as_ref() {
        Some(m) => Ok(m.clone()),
        None => Err(HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "MCP manager not enabled on this server".into(),
            data: None,
        }),
    }
}

/// `mcp/setServers` — register or replace MCP server configurations.
///
/// For each `(name, config_json)` pair in `params.servers`, this
/// handler:
/// 1. Deserializes the JSON value into [`coco_mcp::McpServerConfig`]
///    (transport-tagged enum).
/// 2. Wraps it in a [`coco_mcp::ScopedMcpServerConfig`] with
///    `scope = ConfigScope::Dynamic` and no plugin source.
/// 3. Calls `register_server(...)` on the live manager.
///
/// Note that this only **registers** the configs — it does not
/// auto-connect. Use `mcp/reconnect` (or the existing tool layer's
/// connect-on-first-use logic) to actually establish connections.
///
/// Returns:
/// - `added`: names that were added or replaced
/// - `removed`: always empty in this implementation (no diff vs prior state)
/// - `errors`: per-name deserialization errors
async fn handle_mcp_set_servers(
    params: coco_types::McpSetServersParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let mut manager = manager_arc.lock().await;
    let mut added: Vec<String> = Vec::new();
    let mut errors: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (name, config_json) in params.servers {
        match serde_json::from_value::<coco_mcp::McpServerConfig>(config_json) {
            Ok(config) => {
                let scoped = coco_mcp::ScopedMcpServerConfig {
                    name: name.clone(),
                    config,
                    scope: coco_mcp::ConfigScope::Dynamic,
                    plugin_source: None,
                };
                manager.register_server(scoped);
                added.push(name);
            }
            Err(e) => {
                errors.insert(name, format!("invalid mcp config: {e}"));
            }
        }
    }
    info!(
        added = added.len(),
        errors = errors.len(),
        "SdkServer: mcp/setServers"
    );
    HandlerResult::ok(coco_types::McpSetServersResult {
        added,
        removed: Vec::new(),
        errors,
    })
}

/// `mcp/reconnect` — disconnect + reconnect a specific MCP server.
///
/// Useful after a server's process has been restarted externally or
/// after a transient network failure. The handler unconditionally
/// disconnects (no-op if not connected) then attempts to connect
/// using a no-op elicitation callback.
///
/// Errors:
/// - `INVALID_REQUEST` if MCP manager not enabled
/// - `INTERNAL_ERROR` if the connect attempt fails (e.g. server
///   process refused, OAuth required without elicitation bridge)
async fn handle_mcp_reconnect(
    params: coco_types::McpReconnectParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let manager = manager_arc.lock().await;
    manager.disconnect(&params.server_name).await;
    match manager
        .connect(&params.server_name, no_op_send_elicitation())
        .await
    {
        Ok(()) => {
            info!(server = %params.server_name, "SdkServer: mcp/reconnect ok");
            HandlerResult::ok_empty()
        }
        Err(e) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("mcp/reconnect: {e}"),
            data: None,
        },
    }
}

/// `mcp/toggle` — enable or disable an MCP server.
///
/// `enabled = true`: ensures the server is connected (no-op if
/// already connected).
/// `enabled = false`: disconnects the server.
///
/// Errors:
/// - `INVALID_REQUEST` if MCP manager not enabled
/// - `INTERNAL_ERROR` if enabling and the connect attempt fails
async fn handle_mcp_toggle(
    params: coco_types::McpToggleParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let manager = manager_arc.lock().await;
    if params.enabled {
        match manager
            .connect(&params.server_name, no_op_send_elicitation())
            .await
        {
            Ok(()) => {
                info!(server = %params.server_name, "SdkServer: mcp/toggle (enabled)");
                HandlerResult::ok_empty()
            }
            Err(e) => HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("mcp/toggle enable: {e}"),
                data: None,
            },
        }
    } else {
        manager.disconnect(&params.server_name).await;
        info!(server = %params.server_name, "SdkServer: mcp/toggle (disabled)");
        HandlerResult::ok_empty()
    }
}

#[cfg(test)]
#[path = "handlers.test.rs"]
mod tests;
