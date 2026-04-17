//! Handler-level tests for the SDK server.
//!
//! These tests exercise per-method handler behavior by driving the
//! `SdkServer` dispatch loop over an `InMemoryTransport` and asserting
//! against the resulting wire messages. Tests for dispatcher routing
//! itself (unknown method, parse failure, exit-on-EOF) live in the
//! sibling `dispatcher.test.rs`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use coco_types::CoreEvent;
use coco_types::JsonRpcMessage;
use coco_types::JsonRpcRequest;
use coco_types::RequestId;
use coco_types::ServerNotification;
use coco_types::TurnCompletedParams;
use coco_types::TurnStartedParams;
use coco_types::error_codes;
use pretty_assertions::assert_eq;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::SessionStats;
use super::TurnHandoff;
use super::TurnRunner;
use crate::sdk_server::InMemoryTransport;
use crate::sdk_server::SdkServer;
use crate::sdk_server::SdkTransport;
use crate::sdk_server::handlers::SdkServerState;

// ----- shared test helpers --------------------------------------------

fn req(id: i64, method: &str, params: serde_json::Value) -> JsonRpcMessage {
    JsonRpcMessage::Request(JsonRpcRequest {
        request_id: RequestId::Integer(id),
        method: method.into(),
        params,
    })
}

async fn spawn_server() -> (tokio::task::JoinHandle<()>, Arc<InMemoryTransport>) {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (handle, client_end)
}

async fn spawn_server_with_state() -> (
    tokio::task::JoinHandle<()>,
    Arc<InMemoryTransport>,
    Arc<SdkServerState>,
) {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let state = server.state();
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (handle, client_end, state)
}

async fn spawn_server_with_runner(
    runner: Arc<dyn TurnRunner>,
) -> (tokio::task::JoinHandle<()>, Arc<InMemoryTransport>) {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end).with_turn_runner(runner);
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (handle, client_end)
}

async fn start_session(client: &InMemoryTransport) {
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();
}

/// Build a unique temp directory for tests that need disk persistence.
/// Cleanup is best-effort via a `Drop` guard — we don't leave state
/// between test runs even if a test panics.
struct TempSessionsDir {
    path: std::path::PathBuf,
}

impl TempSessionsDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("coco-sdk-test-sessions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempSessionsDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Spawn a server with a disk-backed SessionManager for list/read/resume
/// tests. Returns the server task, the client transport, the state,
/// and the temp directory guard (drop it to clean up).
async fn spawn_server_with_session_manager() -> (
    tokio::task::JoinHandle<()>,
    Arc<InMemoryTransport>,
    Arc<SdkServerState>,
    TempSessionsDir,
) {
    let tmp = TempSessionsDir::new();
    let manager = Arc::new(coco_session::SessionManager::new(tmp.path.clone()));
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end).with_session_manager(manager);
    let state = server.state();
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (handle, client_end, state, tmp)
}

// ----- mock runners ---------------------------------------------------

/// Emits a scripted `turn/started` → `turn/completed` pair and signals
/// `completed` when done.
struct ScriptedRunner {
    completed: Arc<Notify>,
}

impl TurnRunner for ScriptedRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        _handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        _cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let completed = self.completed.clone();
        Box::pin(async move {
            event_tx
                .send(CoreEvent::Protocol(ServerNotification::TurnStarted(
                    TurnStartedParams {
                        turn_id: Some("scripted".into()),
                        turn_number: 1,
                    },
                )))
                .await
                .ok();
            event_tx
                .send(CoreEvent::Protocol(ServerNotification::TurnCompleted(
                    TurnCompletedParams {
                        turn_id: Some("scripted".into()),
                        usage: coco_types::TokenUsage::default(),
                    },
                )))
                .await
                .ok();
            completed.notify_one();
            Ok(())
        })
    }
}

/// Blocks until cancelled; sets a flag when it observes cancellation.
struct BlockingRunner {
    cancelled: Arc<AtomicBool>,
    started: Arc<Notify>,
}

impl TurnRunner for BlockingRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        _handoff: TurnHandoff,
        _event_tx: mpsc::Sender<CoreEvent>,
        cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let cancelled = self.cancelled.clone();
        let started = self.started.clone();
        Box::pin(async move {
            started.notify_one();
            cancel.cancelled().await;
            cancelled.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Emits a single synthetic per-turn `SessionResult` like the real
/// `QueryEngine` does on each `run_with_events` call. Used to verify
/// the forwarder intercepts them for session-level aggregation.
struct StatsEmittingRunner {
    cost_usd: f64,
    input_tokens: i64,
    output_tokens: i64,
    stop_reason: String,
}

impl TurnRunner for StatsEmittingRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        _handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        _cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let cost = self.cost_usd;
        let input = self.input_tokens;
        let output = self.output_tokens;
        let stop = self.stop_reason.clone();
        Box::pin(async move {
            use coco_types::PermissionDenialInfo;
            use coco_types::SessionResultParams;
            let params = SessionResultParams {
                session_id: "ignored-the-forwarder-overrides".into(),
                total_turns: 1,
                duration_ms: 42,
                duration_api_ms: 30,
                is_error: false,
                stop_reason: stop,
                total_cost_usd: cost,
                usage: coco_types::TokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                model_usage: std::collections::HashMap::new(),
                permission_denials: vec![PermissionDenialInfo {
                    tool_name: "Bash".into(),
                    tool_use_id: "t1".into(),
                    tool_input: serde_json::json!({}),
                }],
                result: Some("hello".into()),
                errors: Vec::new(),
                structured_output: None,
                fast_mode_state: None,
                num_api_calls: Some(1),
            };
            event_tx
                .send(CoreEvent::Protocol(ServerNotification::SessionResult(
                    Box::new(params),
                )))
                .await
                .ok();
            Ok(())
        })
    }
}

// ----- initialize -----------------------------------------------------

#[tokio::test]
async fn initialize_returns_capability_info() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "initialize", serde_json::json!({})))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(1));
            // TS-required fields.
            assert!(r.result["models"].is_array());
            assert!(r.result["models"].as_array().unwrap().len() >= 2);
            assert!(r.result["pid"].is_number());
            assert!(r.result["output_style"].is_string());
            assert!(r.result["available_output_styles"].is_array());
            assert!(r.result["account"].is_object());
            // coco-rs extension fields carried under `_cocoRs*` keys.
            assert_eq!(r.result["_cocoRsProtocolVersion"], "1.0");
            assert!(r.result["_cocoRsVersion"].is_string());
            // Model shape uses TS wire keys (`value`, `displayName`).
            let first_model = &r.result["models"][0];
            assert!(first_model["value"].is_string());
            assert!(first_model["displayName"].is_string());
            assert!(first_model["description"].is_string());
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn initialize_with_bootstrap_returns_real_commands() {
    use crate::sdk_server::CliInitializeBootstrap;
    use coco_commands::CommandRegistry;
    use coco_commands::register_extended_builtins;

    let (server_end, client_end) = InMemoryTransport::pair(32);
    let mut registry = CommandRegistry::new();
    register_extended_builtins(&mut registry);
    let command_count = registry.visible().len();
    assert!(
        command_count > 0,
        "extended built-ins should register at least one visible command"
    );

    let bootstrap: Arc<dyn crate::sdk_server::InitializeBootstrap> = Arc::new(
        CliInitializeBootstrap::new("Explanatory".into()).with_command_registry(Arc::new(registry)),
    );
    let server = SdkServer::new(server_end).with_initialize_bootstrap(bootstrap);
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    client_end
        .send(req(1, "initialize", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client_end.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            let commands = r.result["commands"].as_array().expect("commands array");
            assert_eq!(commands.len(), command_count);
            // Every command has the three TS-required fields.
            for cmd in commands {
                assert!(cmd["name"].is_string());
                assert!(cmd["description"].is_string());
                assert!(cmd["argumentHint"].is_string()); // camelCase on the wire
            }
            assert_eq!(r.result["output_style"], "Explanatory");
            // Built-in output styles always present — TS canonical set
            // is lowercase `default` + capitalized `Explanatory` / `Learning`.
            let styles = r.result["available_output_styles"]
                .as_array()
                .expect("available_output_styles array");
            assert!(styles.iter().any(|s| s == "default"));
            assert!(styles.iter().any(|s| s == "Explanatory"));
            assert!(styles.iter().any(|s| s == "Learning"));
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client_end);
    server_task.await.unwrap();
}

/// Mock [`InitializeBootstrap`] used to exercise the serialization
/// round-trip for `fast_mode_state: Some(...)` — the `CliInitializeBootstrap`
/// impl currently stubs this field to `None`, so a regression in the
/// wire format for non-None values would slip past bootstrap tests.
struct MockFastModeBootstrap {
    state: coco_types::FastModeState,
}

#[async_trait::async_trait]
impl crate::sdk_server::InitializeBootstrap for MockFastModeBootstrap {
    async fn commands(&self) -> Vec<coco_types::SdkSlashCommand> {
        Vec::new()
    }
    async fn agents(&self) -> Vec<coco_types::SdkAgentInfo> {
        Vec::new()
    }
    async fn account(&self) -> coco_types::SdkAccountInfo {
        coco_types::SdkAccountInfo::default()
    }
    async fn output_style(&self) -> String {
        "default".into()
    }
    async fn available_output_styles(&self) -> Vec<String> {
        vec!["default".into()]
    }
    async fn fast_mode_state(&self) -> Option<coco_types::FastModeState> {
        Some(self.state)
    }
}

#[tokio::test]
async fn initialize_fast_mode_state_some_serializes_to_wire() {
    // Exercise each FastModeState variant to guarantee the serde
    // rename emits the TS-canonical lowercase strings (`off` /
    // `cooldown` / `on`) and that a non-None `fast_mode_state` field
    // actually reaches the wire.
    for (state, expected) in [
        (coco_types::FastModeState::Off, "off"),
        (coco_types::FastModeState::Cooldown, "cooldown"),
        (coco_types::FastModeState::On, "on"),
    ] {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let bootstrap: Arc<dyn crate::sdk_server::InitializeBootstrap> =
            Arc::new(MockFastModeBootstrap { state });
        let server = SdkServer::new(server_end).with_initialize_bootstrap(bootstrap);
        let server_task = tokio::spawn(async move {
            let _ = server.run().await;
        });

        client_end
            .send(req(1, "initialize", serde_json::json!({})))
            .await
            .unwrap();
        let reply = client_end.recv().await.unwrap().unwrap();
        match reply {
            JsonRpcMessage::Response(r) => {
                assert_eq!(
                    r.result["fast_mode_state"], expected,
                    "fast_mode_state wire string drift for {state:?}"
                );
            }
            other => panic!("expected Response, got {other:?}"),
        }

        drop(client_end);
        server_task.await.unwrap();
    }
}

#[tokio::test]
async fn initialize_then_session_start_sequence() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "initialize", serde_json::json!({})))
        .await
        .unwrap();
    let init_reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(init_reply, JsonRpcMessage::Response(_)));

    client
        .send(req(2, "session/start", serde_json::json!({ "cwd": "/" })))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    match start_reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(2));
            assert!(r.result["session_id"].is_string());
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- session/start --------------------------------------------------

#[tokio::test]
async fn session_start_returns_session_id() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            2,
            "session/start",
            serde_json::json!({
                "cwd": "/tmp/test",
                "model": "claude-sonnet-4-6",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(2));
            let session_id = r.result["session_id"].as_str().expect("session_id string");
            assert!(session_id.starts_with("session-"));
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_start_rejects_second_concurrent_session() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let first = client.recv().await.unwrap().unwrap();
    assert!(matches!(first, JsonRpcMessage::Response(_)));

    client
        .send(req(2, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let second = client.recv().await.unwrap().unwrap();
    match second {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("already active"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- turn/start + turn/interrupt ------------------------------------

#[tokio::test]
async fn turn_start_rejects_without_active_session() {
    let runner = Arc::new(ScriptedRunner {
        completed: Arc::new(Notify::new()),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    client
        .send(req(1, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no active session"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn turn_start_returns_turn_id_and_forwards_notifications() {
    let completed = Arc::new(Notify::new());
    let runner = Arc::new(ScriptedRunner {
        completed: completed.clone(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();

    let mut turn_start_reply: Option<coco_types::JsonRpcResponse> = None;
    let mut notif_methods: Vec<String> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline
        && (turn_start_reply.is_none() || notif_methods.len() < 2)
    {
        match tokio::time::timeout(Duration::from_millis(500), client.recv()).await {
            Ok(Ok(Some(JsonRpcMessage::Response(r)))) if r.request_id == RequestId::Integer(2) => {
                turn_start_reply = Some(r);
            }
            Ok(Ok(Some(JsonRpcMessage::Notification(n)))) => {
                notif_methods.push(n.method);
            }
            _ => continue,
        }
    }

    let reply = turn_start_reply.expect("turn/start response not seen");
    let turn_id = reply.result["turn_id"].as_str().expect("turn_id string");
    assert!(turn_id.starts_with("turn-session-"));
    assert_eq!(
        notif_methods,
        vec!["turn/started".to_string(), "turn/completed".to_string()]
    );

    tokio::time::timeout(Duration::from_secs(1), completed.notified())
        .await
        .expect("runner should complete");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn turn_interrupt_cancels_running_turn() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Notify::new());
    let runner = Arc::new(BlockingRunner {
        cancelled: cancelled.clone(),
        started: started.clone(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .expect("runner should start");

    client
        .send(req(3, "turn/interrupt", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(3));
            assert!(r.result.is_null());
        }
        other => panic!("expected Response, got {other:?}"),
    }

    for _ in 0..20 {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        cancelled.load(Ordering::SeqCst),
        "runner should have observed cancellation"
    );

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn turn_interrupt_without_active_turn_errors() {
    let runner = Arc::new(ScriptedRunner {
        completed: Arc::new(Notify::new()),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(2, "turn/interrupt", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no turn in flight"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn turn_start_rejects_second_concurrent_turn() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Notify::new());
    let runner = Arc::new(BlockingRunner {
        cancelled: cancelled.clone(),
        started: started.clone(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(
            2,
            "turn/start",
            serde_json::json!({ "prompt": "first" }),
        ))
        .await
        .unwrap();
    let first = client.recv().await.unwrap().unwrap();
    assert!(matches!(first, JsonRpcMessage::Response(_)));

    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .expect("first turn should start");

    client
        .send(req(
            3,
            "turn/start",
            serde_json::json!({ "prompt": "second" }),
        ))
        .await
        .unwrap();
    let second = client.recv().await.unwrap().unwrap();
    match second {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("already running"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    client
        .send(req(4, "turn/interrupt", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap();

    drop(client);
    server_task.await.unwrap();
}

// ----- approval/resolve + input/resolveUserInput ----------------------

#[tokio::test]
async fn approval_resolve_delivers_decision_to_pending_receiver() {
    use coco_types::ApprovalDecision;

    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_approval("req-1".into()).await;

    client
        .send(req(
            1,
            "approval/resolve",
            serde_json::json!({
                "request_id": "req-1",
                "decision": "allow",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(1));
            assert!(r.result.is_null());
        }
        other => panic!("expected Response, got {other:?}"),
    }

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert_eq!(params.request_id, "req-1");
    assert_eq!(params.decision, ApprovalDecision::Allow);

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn approval_resolve_unknown_id_errors() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "approval/resolve",
            serde_json::json!({
                "request_id": "does-not-exist",
                "decision": "deny",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no pending approval"));
            assert!(e.message.contains("does-not-exist"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn approval_resolve_tolerates_dropped_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_approval("req-orphan".into()).await;
    drop(rx);

    client
        .send(req(
            1,
            "approval/resolve",
            serde_json::json!({
                "request_id": "req-orphan",
                "decision": "allow",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn user_input_resolve_delivers_answer_to_pending_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_user_input("q-1".into()).await;

    client
        .send(req(
            1,
            "input/resolveUserInput",
            serde_json::json!({
                "request_id": "q-1",
                "answer": "option 2",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert_eq!(params.request_id, "q-1");
    assert_eq!(params.answer, "option 2");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn user_input_resolve_unknown_id_errors() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "input/resolveUserInput",
            serde_json::json!({
                "request_id": "missing",
                "answer": "x",
            }),
        ))
        .await
        .unwrap();

    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no pending user input"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- session/archive + runtime control ------------------------------

#[tokio::test]
async fn session_archive_clears_active_session() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_id = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    client
        .send(req(
            2,
            "session/archive",
            serde_json::json!({ "session_id": session_id.clone() }),
        ))
        .await
        .unwrap();

    // Drain the aggregated notification + response (order-agnostic).
    let mut saw_notif = false;
    let mut saw_response = false;
    for _ in 0..2 {
        let msg = client.recv().await.unwrap().unwrap();
        match msg {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "session/result");
                assert_eq!(n.params["session_id"], session_id);
                assert_eq!(n.params["total_turns"], 0);
                assert_eq!(n.params["is_error"], false);
                saw_notif = true;
            }
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.request_id, RequestId::Integer(2));
                assert!(r.result.is_null());
                saw_response = true;
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
    assert!(saw_notif && saw_response);

    client
        .send(req(3, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_archive_rejects_mismatched_id() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    client
        .send(req(
            2,
            "session/archive",
            serde_json::json!({ "session_id": "session-does-not-exist" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("session_id mismatch"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_archive_without_active_session_errors() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            1,
            "session/archive",
            serde_json::json!({ "session_id": "whatever" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no active session"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn set_model_updates_session_model() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    client
        .send(req(
            2,
            "control/setModel",
            serde_json::json!({ "model": "claude-sonnet-4-6" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let slot = state.session.read().await;
    assert_eq!(slot.as_ref().unwrap().model, "claude-sonnet-4-6");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn set_model_with_null_reverts_to_default() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    client
        .send(req(2, "control/setModel", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    let slot = state.session.read().await;
    assert_eq!(slot.as_ref().unwrap().model, "claude-opus-4-6");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn set_permission_mode_updates_session_field() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    client
        .send(req(
            2,
            "control/setPermissionMode",
            serde_json::json!({ "mode": "plan" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let slot = state.session.read().await;
    assert!(matches!(
        slot.as_ref().unwrap().permission_mode,
        Some(coco_types::PermissionMode::Plan)
    ));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn set_thinking_updates_session_field() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    client
        .send(req(
            2,
            "control/setThinking",
            serde_json::json!({
                "thinking_level": {
                    "effort": "high",
                    "budget_tokens": 4096,
                }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let slot = state.session.read().await;
    let tl = slot.as_ref().unwrap().thinking_level.as_ref().unwrap();
    assert_eq!(tl.effort, coco_types::ReasoningEffort::High);
    assert_eq!(tl.budget_tokens, Some(4096));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn stop_task_cancels_running_turn() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Notify::new());
    let runner = Arc::new(BlockingRunner {
        cancelled: cancelled.clone(),
        started: started.clone(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .expect("runner should start");

    client
        .send(req(
            3,
            "control/stopTask",
            serde_json::json!({ "task_id": "some-task" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    for _ in 0..20 {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(cancelled.load(Ordering::SeqCst));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn update_env_merges_and_clears() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    client
        .send(req(
            2,
            "control/updateEnv",
            serde_json::json!({
                "env": { "FOO": "1", "BAR": "2" }
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap();

    {
        let slot = state.session.read().await;
        let env = &slot.as_ref().unwrap().env_overrides;
        assert_eq!(env.get("FOO").map(String::as_str), Some("1"));
        assert_eq!(env.get("BAR").map(String::as_str), Some("2"));
    }

    client
        .send(req(
            3,
            "control/updateEnv",
            serde_json::json!({
                "env": { "BAZ": "3", "FOO": "" }
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap();

    let slot = state.session.read().await;
    let env = &slot.as_ref().unwrap().env_overrides;
    assert!(!env.contains_key("FOO"));
    assert_eq!(env.get("BAR").map(String::as_str), Some("2"));
    assert_eq!(env.get("BAZ").map(String::as_str), Some("3"));

    drop(slot);
    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn cancel_request_removes_pending_approval() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_approval("req-42".into()).await;

    client
        .send(req(
            1,
            "control/cancelRequest",
            serde_json::json!({ "request_id": "req-42", "reason": "user closed prompt" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let drained = rx.await;
    assert!(drained.is_err(), "sender should have been dropped");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn cancel_request_on_unknown_id_still_ok() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "control/cancelRequest",
            serde_json::json!({ "request_id": "never-was" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.7: session envelope fix ------------------------------

#[tokio::test]
async fn session_result_from_runner_is_intercepted_not_forwarded() {
    let runner = Arc::new(StatsEmittingRunner {
        cost_usd: 0.01,
        input_tokens: 100,
        output_tokens: 50,
        stop_reason: "end_turn".into(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();

    let turn_reply = client.recv().await.unwrap().unwrap();
    match turn_reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(2));
            assert!(r.result["turn_id"].is_string());
        }
        other => panic!("expected turn/start response, got {other:?}"),
    }

    // Verify nothing else leaked through: the next keepAlive response
    // should come straight back with no intervening session/result.
    client
        .send(req(3, "control/keepAlive", serde_json::json!({})))
        .await
        .unwrap();
    let next = client.recv().await.unwrap().unwrap();
    match next {
        JsonRpcMessage::Response(r) => {
            assert_eq!(
                r.request_id,
                RequestId::Integer(3),
                "saw something other than the keepAlive response — SessionResult leaked?"
            );
        }
        other => panic!("unexpected message: {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_archive_emits_aggregated_session_result() {
    let runner = Arc::new(StatsEmittingRunner {
        cost_usd: 0.02,
        input_tokens: 200,
        output_tokens: 80,
        stop_reason: "end_turn".into(),
    });
    let (server_task, client, state) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let state = server.state();
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end, state)
    };

    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_id = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    for id in [2, 3] {
        client
            .send(req(id, "turn/start", serde_json::json!({ "prompt": "hi" })))
            .await
            .unwrap();
        let _ = client.recv().await.unwrap().unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    {
        let slot = state.session.read().await;
        let stats = &slot.as_ref().unwrap().stats;
        assert_eq!(stats.total_turns, 2);
        assert_eq!(stats.usage.input_tokens, 400);
        assert_eq!(stats.usage.output_tokens, 160);
        assert!((stats.total_cost_usd - 0.04).abs() < 1e-9);
        assert_eq!(stats.permission_denials.len(), 2);
        assert_eq!(stats.last_stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(stats.last_result_text.as_deref(), Some("hello"));
    }

    client
        .send(req(
            4,
            "session/archive",
            serde_json::json!({ "session_id": session_id.clone() }),
        ))
        .await
        .unwrap();

    let mut saw_notif = false;
    let mut saw_response = false;
    for _ in 0..2 {
        let msg = client.recv().await.unwrap().unwrap();
        match msg {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "session/result");
                assert_eq!(n.params["session_id"], session_id);
                assert_eq!(n.params["total_turns"], 2);
                assert_eq!(n.params["usage"]["input_tokens"], 400);
                assert_eq!(n.params["usage"]["output_tokens"], 160);
                assert_eq!(n.params["permission_denials"].as_array().unwrap().len(), 2);
                assert_eq!(n.params["result"], "hello");
                assert_eq!(n.params["stop_reason"], "end_turn");
                saw_notif = true;
            }
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.request_id, RequestId::Integer(4));
                saw_response = true;
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
    assert!(saw_notif && saw_response);

    drop(client);
    server_task.await.unwrap();
}

/// Runner that emits a late `TurnFailed` event AFTER observing cancel
/// and BEFORE returning from `run_turn`. Used to exercise the archive
/// ordering contract: the late event must arrive on the wire BEFORE
/// the aggregated `SessionResult`, since archive waits for the runner
/// + forwarder to drain before emitting its aggregate.
struct LateEventOnCancelRunner {
    started: Arc<Notify>,
}

impl TurnRunner for LateEventOnCancelRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        _handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let started = self.started.clone();
        Box::pin(async move {
            started.notify_one();
            // Wait for the cancel signal (archive triggers it).
            cancel.cancelled().await;
            // Emit a late event POST-cancel — archive must flush this
            // before emitting its own aggregated SessionResult.
            let _ = event_tx
                .send(CoreEvent::Protocol(ServerNotification::TurnFailed(
                    coco_types::TurnFailedParams {
                        error: "cancelled mid-turn".into(),
                    },
                )))
                .await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn session_archive_flushes_late_events_before_aggregate() {
    // Archive must wait for the runner + forwarder to drain so any
    // events emitted after the cancel token fires (but before the
    // runner exits) land on the wire BEFORE the aggregated
    // `SessionResult`. Otherwise the client sees:
    //     session/result (aggregated, archived)
    //     turn/failed (cancelled mid-turn)   ← out of order
    // which confuses strict event-order consumers.
    let started = Arc::new(Notify::new());
    let runner = Arc::new(LateEventOnCancelRunner {
        started: started.clone(),
    });
    let (server_task, client) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end)
    };

    // Start session + turn.
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let session_id = match client.recv().await.unwrap().unwrap() {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "hi" })))
        .await
        .unwrap();
    // turn/start response (sync).
    let _ = client.recv().await.unwrap().unwrap();

    // Wait for the runner to actually start before archiving.
    started.notified().await;

    // Archive while the turn is blocked on cancel. Archive cancels the
    // token, the runner emits its late TurnFailed, the forwarder pushes
    // it onto the wire, then archive emits the aggregate.
    client
        .send(req(
            3,
            "session/archive",
            serde_json::json!({ "session_id": session_id }),
        ))
        .await
        .unwrap();

    // Drain messages until we see the archive response, collecting all
    // notifications in order. Assert that TurnFailed arrives BEFORE
    // SessionResult.
    let mut observed_kinds: Vec<&'static str> = Vec::new();
    loop {
        let msg = client.recv().await.unwrap().unwrap();
        match msg {
            JsonRpcMessage::Notification(n) => {
                if n.method == "turn/failed" {
                    observed_kinds.push("turn/failed");
                } else if n.method == "session/result" {
                    observed_kinds.push("session/result");
                }
            }
            JsonRpcMessage::Response(r) if r.request_id == RequestId::Integer(3) => break,
            _ => continue,
        }
    }

    // TurnFailed must appear before session/result on the wire.
    let late_pos = observed_kinds.iter().position(|k| *k == "turn/failed");
    let result_pos = observed_kinds.iter().position(|k| *k == "session/result");
    assert!(
        late_pos.is_some() && result_pos.is_some(),
        "expected both turn/failed and session/result notifications, got {observed_kinds:?}"
    );
    assert!(
        late_pos < result_pos,
        "expected turn/failed BEFORE session/result, got {observed_kinds:?}"
    );

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.8: ServerRequest emission round-trip -----------------

#[tokio::test]
async fn send_server_request_roundtrips_success() {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let state = server.state();
    let transport = server.transport();
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    let state_for_req = state.clone();
    let transport_for_req = transport.clone();
    let send_task = tokio::spawn(async move {
        state_for_req
            .send_server_request(
                &transport_for_req,
                "approval/askForApproval",
                serde_json::json!({
                    "request_id": "r-abc",
                    "tool_name": "Bash",
                    "input": { "command": "ls" },
                    "tool_use_id": "tu-1",
                }),
            )
            .await
    });

    let incoming = client_end.recv().await.unwrap().unwrap();
    let server_req_id = match incoming {
        JsonRpcMessage::Request(r) => {
            assert_eq!(r.method, "approval/askForApproval");
            assert_eq!(r.params["tool_name"], "Bash");
            r.request_id
        }
        other => panic!("expected Request, got {other:?}"),
    };
    client_end
        .send(JsonRpcMessage::Response(coco_types::JsonRpcResponse {
            request_id: server_req_id,
            result: serde_json::json!({ "decision": "allow" }),
        }))
        .await
        .unwrap();

    let reply = send_task.await.unwrap().expect("server request succeeded");
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["decision"], "allow");
        }
        other => panic!("expected Response, got {other:?}"),
    }

    assert!(state.pending_server_requests.lock().await.is_empty());

    drop(client_end);
    server_task.await.unwrap();
}

#[tokio::test]
async fn send_server_request_returns_error_on_error_reply() {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let state = server.state();
    let transport = server.transport();
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    let state_for_req = state.clone();
    let transport_for_req = transport.clone();
    let send_task = tokio::spawn(async move {
        state_for_req
            .send_server_request(
                &transport_for_req,
                "approval/askForApproval",
                serde_json::json!({}),
            )
            .await
    });

    let incoming = client_end.recv().await.unwrap().unwrap();
    let id = match incoming {
        JsonRpcMessage::Request(r) => r.request_id,
        other => panic!("expected Request, got {other:?}"),
    };
    client_end
        .send(JsonRpcMessage::Error(coco_types::JsonRpcError {
            request_id: id,
            code: error_codes::INTERNAL_ERROR,
            message: "client says no".into(),
            data: None,
        }))
        .await
        .unwrap();

    let reply = send_task.await.unwrap().expect("send returned");
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INTERNAL_ERROR);
            assert!(e.message.contains("client says no"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client_end);
    server_task.await.unwrap();
}

#[tokio::test]
async fn send_server_request_unique_ids() {
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let state = server.state();
    let transport = server.transport();
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    let state_a = state.clone();
    let transport_a = transport.clone();
    let a = tokio::spawn(async move {
        state_a
            .send_server_request(&transport_a, "method/a", serde_json::json!({}))
            .await
    });
    let state_b = state.clone();
    let transport_b = transport.clone();
    let b = tokio::spawn(async move {
        state_b
            .send_server_request(&transport_b, "method/b", serde_json::json!({}))
            .await
    });

    let req1 = match client_end.recv().await.unwrap().unwrap() {
        JsonRpcMessage::Request(r) => r,
        other => panic!("{other:?}"),
    };
    let req2 = match client_end.recv().await.unwrap().unwrap() {
        JsonRpcMessage::Request(r) => r,
        other => panic!("{other:?}"),
    };
    assert_ne!(req1.request_id, req2.request_id);

    client_end
        .send(JsonRpcMessage::Response(coco_types::JsonRpcResponse {
            request_id: req1.request_id.clone(),
            result: serde_json::json!({ "method": req1.method }),
        }))
        .await
        .unwrap();
    client_end
        .send(JsonRpcMessage::Response(coco_types::JsonRpcResponse {
            request_id: req2.request_id.clone(),
            result: serde_json::json!({ "method": req2.method }),
        }))
        .await
        .unwrap();

    let reply_a = a.await.unwrap().unwrap();
    let reply_b = b.await.unwrap().unwrap();
    assert!(matches!(reply_a, JsonRpcMessage::Response(_)));
    assert!(matches!(reply_b, JsonRpcMessage::Response(_)));

    drop(client_end);
    server_task.await.unwrap();
}

// ----- SessionStats sanity --------------------------------------------

#[test]
fn session_stats_default_is_zero() {
    let stats = SessionStats::default();
    assert_eq!(stats.total_turns, 0);
    assert_eq!(stats.total_cost_usd, 0.0);
    assert!(stats.model_usage.is_empty());
    assert!(stats.permission_denials.is_empty());
    assert!(!stats.had_error);
}

// ----- Phase 2.C.10: multi-turn context persistence -------------------

/// Runner that observes `session.history.len()` before mutating, then
/// appends two synthetic messages (a user + an assistant). If history
/// is correctly threaded across turn/start calls, successive runs will
/// observe monotonically growing lengths.
struct HistoryRecordingRunner {
    observed_prior_lens: Arc<tokio::sync::Mutex<Vec<usize>>>,
}

impl TurnRunner for HistoryRecordingRunner {
    fn run_turn<'a>(
        &'a self,
        params: coco_types::TurnStartParams,
        handoff: TurnHandoff,
        _event_tx: mpsc::Sender<CoreEvent>,
        _cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let observed = self.observed_prior_lens.clone();
        Box::pin(async move {
            let prior_len = {
                let h = handoff.history.lock().await;
                h.len()
            };
            observed.lock().await.push(prior_len);
            let user = coco_messages::create_user_message(&params.prompt);
            let assistant = coco_messages::create_user_message(&format!(
                "(simulated reply to: {})",
                params.prompt
            ));
            let mut h = handoff.history.lock().await;
            h.push(user);
            h.push(assistant);
            Ok(())
        })
    }
}

#[tokio::test]
async fn multi_turn_threads_history_between_turn_starts() {
    let observed: Arc<tokio::sync::Mutex<Vec<usize>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let runner = Arc::new(HistoryRecordingRunner {
        observed_prior_lens: observed.clone(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    for id in [2, 3, 4] {
        client
            .send(req(
                id,
                "turn/start",
                serde_json::json!({ "prompt": format!("turn {id}") }),
            ))
            .await
            .unwrap();
        let reply = client.recv().await.unwrap().unwrap();
        assert!(matches!(reply, JsonRpcMessage::Response(_)));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    // Three turns fired; each runner observed the prior turn's +2
    // messages, so the observed lens should be 0, 2, 4.
    let snapshot = observed.lock().await.clone();
    assert_eq!(snapshot, vec![0, 2, 4]);

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_start_initializes_empty_history() {
    let (server_task, client, state) = spawn_server_with_state().await;

    start_session(&client).await;

    {
        let slot = state.session.read().await;
        let history = &slot.as_ref().unwrap().history;
        assert!(history.lock().await.is_empty());
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- Second-round fix regression tests ------------------------------

#[tokio::test]
async fn session_archive_is_atomic_under_concurrent_forwarder_updates() {
    // Second-round fix regression test for the TOCTOU race:
    //
    // Before the fix, `handle_session_archive` took a read lock, built
    // the aggregate, dropped the lock, then took a write lock to clear.
    // Between the read and write locks, the per-turn forwarder could
    // run `accumulate_session_result`, which silently added stats that
    // were never reflected in the emitted aggregate.
    //
    // After the fix, archive holds the write lock for the entire
    // operation — validate → build aggregate → clear slot — so no
    // other mutation can interleave. This test verifies the aggregate
    // reflects all stats accumulated up to the point where archive
    // acquires the lock, and that subsequent forwarder attempts are
    // no-ops (session slot is None).

    let runner = Arc::new(StatsEmittingRunner {
        cost_usd: 0.05,
        input_tokens: 500,
        output_tokens: 250,
        stop_reason: "end_turn".into(),
    });
    let (server_task, client, _state) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let state = server.state();
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end, state)
    };

    // Start session
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_id = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    // Fire one turn, let it complete + stats fold in.
    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "go" })))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Archive — the emitted aggregate must include the single turn's stats.
    client
        .send(req(
            3,
            "session/archive",
            serde_json::json!({ "session_id": session_id.clone() }),
        ))
        .await
        .unwrap();

    let mut saw_notif = false;
    let mut saw_response = false;
    for _ in 0..2 {
        let msg = client.recv().await.unwrap().unwrap();
        match msg {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "session/result");
                // Stats from the single turn must be present.
                assert_eq!(n.params["total_turns"], 1);
                assert_eq!(n.params["usage"]["input_tokens"], 500);
                assert_eq!(n.params["usage"]["output_tokens"], 250);
                assert_eq!(n.params["permission_denials"].as_array().unwrap().len(), 1);
                saw_notif = true;
            }
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.request_id, RequestId::Integer(3));
                saw_response = true;
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
    assert!(saw_notif && saw_response);

    drop(client);
    server_task.await.unwrap();
}

/// Runner that always returns `Err(...)` from `run_turn` WITHOUT
/// going through the engine's normal `make_result` path. Used to
/// verify engine-bail-style failures still record stats via the
/// forwarder synthetic-SessionResult mechanism.
struct ErrorRunner;

impl TurnRunner for ErrorRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        _cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            // Emulate what `QueryEngineRunner` does on an engine-bail
            // path: emit a synthetic SessionResult with is_error=true
            // and then return Err.
            let params = coco_types::SessionResultParams {
                session_id: handoff.session_id.clone(),
                total_turns: 1,
                duration_ms: 0,
                duration_api_ms: 0,
                is_error: true,
                stop_reason: "engine_error".into(),
                total_cost_usd: 0.0,
                usage: coco_types::TokenUsage::default(),
                model_usage: std::collections::HashMap::new(),
                permission_denials: Vec::new(),
                result: None,
                errors: vec!["fake engine crash".into()],
                structured_output: None,
                fast_mode_state: None,
                num_api_calls: None,
            };
            let _ = event_tx
                .send(CoreEvent::Protocol(ServerNotification::SessionResult(
                    Box::new(params),
                )))
                .await;
            // Give the forwarder a beat to process.
            tokio::time::sleep(Duration::from_millis(20)).await;
            anyhow::bail!("fake engine crash")
        })
    }
}

// ----- Third-round fix: cross-session contamination ------------------

/// Runner that takes a long time (awaits an external unblock signal)
/// before returning. Used to simulate a turn that's still winding down
/// when `session/archive` + `session/start` replaces the active
/// session — the test can then verify neither (a) the turn's cleanup
/// nor (b) its forwarder's stat accumulation corrupts the new session.
struct SlowRunner {
    started: Arc<Notify>,
    unblock: Arc<Notify>,
    /// Whether to emit a synthetic SessionResult just before exit.
    /// Used to exercise the forwarder's cross-session guard.
    emit_session_result: bool,
}

impl TurnRunner for SlowRunner {
    fn run_turn<'a>(
        &'a self,
        _params: coco_types::TurnStartParams,
        handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let started = self.started.clone();
        let unblock = self.unblock.clone();
        let emit = self.emit_session_result;
        Box::pin(async move {
            started.notify_one();
            // Honor the cancel token so `session/archive`'s join-before-
            // emit path doesn't wait for the 5s timeout. If cancel fires
            // first, skip the synthetic emit and exit immediately — that
            // matches real runners that abort mid-turn on cancel.
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Ok(());
                }
                _ = unblock.notified() => {}
            }
            if emit {
                let params = coco_types::SessionResultParams {
                    session_id: handoff.session_id.clone(),
                    total_turns: 1,
                    duration_ms: 0,
                    duration_api_ms: 0,
                    is_error: false,
                    stop_reason: "end_turn".into(),
                    total_cost_usd: 99.0, // distinctive contamination marker
                    usage: coco_types::TokenUsage {
                        input_tokens: 9999,
                        output_tokens: 9999,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                    model_usage: std::collections::HashMap::new(),
                    permission_denials: Vec::new(),
                    result: Some("ghost turn reply".into()),
                    errors: Vec::new(),
                    structured_output: None,
                    fast_mode_state: None,
                    num_api_calls: None,
                };
                let _ = event_tx
                    .send(CoreEvent::Protocol(ServerNotification::SessionResult(
                        Box::new(params),
                    )))
                    .await;
                // Give the forwarder time to process before exit.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn turn_cleanup_does_not_corrupt_successor_session_cancel_state() {
    // Regression test for Fix #A3: when session/archive + session/start
    // races with a turn's cleanup, the cleanup must NOT null out the
    // new session's `active_turn_cancel`.
    let started = Arc::new(Notify::new());
    let unblock = Arc::new(Notify::new());
    let runner = Arc::new(SlowRunner {
        started: started.clone(),
        unblock: unblock.clone(),
        emit_session_result: false,
    });
    let (server_task, client, state) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let state = server.state();
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end, state)
    };

    // Session A + turn T_A
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_a = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    client
        .send(req(
            2,
            "turn/start",
            serde_json::json!({ "prompt": "slow" }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .expect("T_A should start");

    // Archive A while T_A is still running.
    client
        .send(req(
            3,
            "session/archive",
            serde_json::json!({ "session_id": session_a.clone() }),
        ))
        .await
        .unwrap();
    for _ in 0..2 {
        let _ = client.recv().await.unwrap().unwrap();
    }

    // Start session B.
    client
        .send(req(4, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    // Manually install a fresh cancel token on B's active_turn_cancel
    // so we can verify it survives T_A's cleanup.
    {
        let mut slot = state.session.write().await;
        let session = slot.as_mut().unwrap();
        session.active_turn_cancel = Some(CancellationToken::new());
    }

    // Now unblock T_A so its cleanup runs.
    unblock.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify session B's active_turn_cancel is still Some.
    {
        let slot = state.session.read().await;
        let session = slot.as_ref().unwrap();
        assert!(
            session.active_turn_cancel.is_some(),
            "session B's cancel token was wrongly cleared by session A's turn cleanup"
        );
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn forwarder_does_not_contaminate_successor_session_stats() {
    // Regression test for Fix #F3: a forwarder processing an event
    // from a now-dead session must NOT fold its stats into the new
    // session that has taken the slot.
    let started = Arc::new(Notify::new());
    let unblock = Arc::new(Notify::new());
    let runner = Arc::new(SlowRunner {
        started: started.clone(),
        unblock: unblock.clone(),
        emit_session_result: true,
    });
    let (server_task, client, state) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let state = server.state();
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end, state)
    };

    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_a = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    client
        .send(req(
            2,
            "turn/start",
            serde_json::json!({ "prompt": "slow" }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .expect("T_A should start");

    // Archive A.
    client
        .send(req(
            3,
            "session/archive",
            serde_json::json!({ "session_id": session_a }),
        ))
        .await
        .unwrap();
    for _ in 0..2 {
        let _ = client.recv().await.unwrap().unwrap();
    }

    // Start session B (fresh stats).
    client
        .send(req(4, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    // Unblock T_A — it emits its ghost SessionResult AFTER B is
    // active. Without Fix #F3, the forwarder would fold 9999 tokens
    // + $99 cost into B's stats.
    unblock.notify_one();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // B's stats must be pristine.
    {
        let slot = state.session.read().await;
        let stats = &slot.as_ref().unwrap().stats;
        assert_eq!(stats.total_turns, 0, "B's total_turns contaminated");
        assert_eq!(
            stats.usage.input_tokens, 0,
            "B's input_tokens contaminated: got {}",
            stats.usage.input_tokens
        );
        assert_eq!(stats.total_cost_usd, 0.0, "B's cost contaminated");
        assert!(stats.last_result_text.is_none(), "B's result contaminated");
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- Third-round fix: pending request leak on cancel ---------------

#[tokio::test]
async fn send_server_request_cleans_up_pending_on_receiver_drop() {
    // Regression test for Fix #L: if the caller wraps
    // `send_server_request` in a `tokio::select!` and the cancel
    // branch fires before a reply arrives, the pending_server_requests
    // entry must be removed by the PendingRequestGuard. Without the
    // guard, the entry would leak in the map until state drop.
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end);
    let state = server.state();
    let transport = server.transport();
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    let state_for_send = state.clone();
    let transport_for_send = transport.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let send_task = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = stop_rx => "cancelled",
            _ = state_for_send.send_server_request(
                &transport_for_send,
                "approval/askForApproval",
                serde_json::json!({}),
            ) => "replied",
        }
    });

    // Drain the outgoing request from the client side so the transport
    // write completes; otherwise send_server_request blocks in the send.
    let _ = client_end.recv().await.unwrap().unwrap();

    // Verify the entry exists.
    {
        let map = state.pending_server_requests.lock().await;
        assert_eq!(map.len(), 1, "expected one pending entry before cancel");
    }

    // Cancel the send (simulate the outer select!'s cancel branch).
    let _ = stop_tx.send(());
    let outcome = send_task.await.unwrap();
    assert_eq!(outcome, "cancelled");

    // Drop guard should have removed the entry.
    tokio::time::sleep(Duration::from_millis(20)).await;
    {
        let map = state.pending_server_requests.lock().await;
        assert!(
            map.is_empty(),
            "PendingRequestGuard must clean up the pending entry on cancel"
        );
    }

    drop(client_end);
    server_task.await.unwrap();
}

#[tokio::test]
async fn engine_error_is_recorded_in_session_stats() {
    // Second-round fix regression test: when the runner's engine.run
    // returns Err (not via make_result), the runner now emits a
    // synthetic SessionResult{is_error=true} so the forwarder folds
    // the failure into session.stats.had_error. Without this, true
    // engine-bail paths would silently produce an aggregated
    // SessionResult with `is_error=false`.

    let runner = Arc::new(ErrorRunner);
    let (server_task, client, state) = {
        let (server_end, client_end) = InMemoryTransport::pair(32);
        let server = SdkServer::new(server_end).with_turn_runner(runner);
        let state = server.state();
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        (handle, client_end, state)
    };

    // Start session + fire a doomed turn.
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_id = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    client
        .send(req(
            2,
            "turn/start",
            serde_json::json!({ "prompt": "crash" }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    // Wait for the runner to emit its synthetic error SessionResult
    // and for the forwarder to fold it into stats.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify stats reflect the engine error.
    {
        let slot = state.session.read().await;
        let stats = &slot.as_ref().unwrap().stats;
        assert_eq!(stats.total_turns, 1);
        assert!(stats.had_error);
        assert_eq!(stats.errors.len(), 1);
        assert!(stats.errors[0].contains("fake engine crash"));
    }

    // Archive — the aggregated SessionResult must reflect is_error=true.
    client
        .send(req(
            3,
            "session/archive",
            serde_json::json!({ "session_id": session_id }),
        ))
        .await
        .unwrap();

    let mut saw_error_result = false;
    for _ in 0..2 {
        let msg = client.recv().await.unwrap().unwrap();
        if let JsonRpcMessage::Notification(n) = msg
            && n.method == "session/result"
        {
            assert_eq!(n.params["is_error"], true);
            assert_eq!(n.params["errors"][0], "fake engine crash");
            saw_error_result = true;
        }
    }
    assert!(saw_error_result);

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.11: session/list + session/read + session/resume ---

#[tokio::test]
async fn session_list_without_manager_returns_empty() {
    // If no SessionManager is wired (default), session/list returns
    // an empty list rather than erroring. This matches the
    // "persistence disabled" semantics.
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "session/list", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["sessions"].as_array().unwrap().len(), 0);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_list_returns_persisted_sessions() {
    let (server_task, client, state, _tmp) = spawn_server_with_session_manager().await;

    // Start two sessions and archive each to get them persisted +
    // removed. Wait — archive deletes. So to get a persisted session
    // visible to list, we start one and DON'T archive.
    //
    // We also can't start two at once (single-session semantics).
    // Instead, pre-populate the manager directly with two records,
    // then call session/list.
    let manager = state.session_manager.read().await.clone().unwrap();
    manager
        .save(&coco_session::Session {
            id: "pre-existing-a".into(),
            created_at: "100".into(),
            updated_at: None,
            model: "claude-opus-4-6".into(),
            working_dir: std::path::PathBuf::from("/tmp/a"),
            title: Some("first".into()),
            message_count: 3,
            total_tokens: 1000,
        })
        .unwrap();
    manager
        .save(&coco_session::Session {
            id: "pre-existing-b".into(),
            created_at: "200".into(),
            updated_at: None,
            model: "claude-sonnet-4-6".into(),
            working_dir: std::path::PathBuf::from("/tmp/b"),
            title: None,
            message_count: 0,
            total_tokens: 0,
        })
        .unwrap();

    client
        .send(req(1, "session/list", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            let sessions = r.result["sessions"].as_array().unwrap();
            assert_eq!(sessions.len(), 2);
            // Newest first — created_at "200" > "100".
            assert_eq!(sessions[0]["session_id"], "pre-existing-b");
            assert_eq!(sessions[1]["session_id"], "pre-existing-a");
            assert_eq!(sessions[1]["title"], "first");
            assert_eq!(sessions[1]["message_count"], 3);
            assert_eq!(sessions[1]["total_tokens"], 1000);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_start_persists_session_to_disk() {
    let (server_task, client, state, _tmp) = spawn_server_with_session_manager().await;

    client
        .send(req(
            1,
            "session/start",
            serde_json::json!({ "cwd": "/tmp/foo", "model": "claude-sonnet-4-6" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    let session_id = match reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    // Verify the session was saved to disk.
    let manager = state.session_manager.read().await.clone().unwrap();
    let loaded = manager
        .load(&session_id)
        .expect("session should be persisted");
    assert_eq!(loaded.id, session_id);
    assert_eq!(loaded.model, "claude-sonnet-4-6");
    assert_eq!(loaded.working_dir, std::path::PathBuf::from("/tmp/foo"));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_read_returns_metadata_for_persisted_session() {
    let (server_task, client, state, _tmp) = spawn_server_with_session_manager().await;

    let manager = state.session_manager.read().await.clone().unwrap();
    manager
        .save(&coco_session::Session {
            id: "read-test".into(),
            created_at: "123".into(),
            updated_at: Some("456".into()),
            model: "claude-opus-4-6".into(),
            working_dir: std::path::PathBuf::from("/tmp/read"),
            title: Some("my session".into()),
            message_count: 7,
            total_tokens: 5000,
        })
        .unwrap();

    client
        .send(req(
            1,
            "session/read",
            serde_json::json!({ "session_id": "read-test" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["session"]["session_id"], "read-test");
            assert_eq!(r.result["session"]["title"], "my session");
            assert_eq!(r.result["session"]["message_count"], 7);
            assert_eq!(r.result["session"]["total_tokens"], 5000);
            // Phase 2.C.11 doesn't return messages yet.
            assert_eq!(r.result["messages"].as_array().unwrap().len(), 0);
            assert_eq!(r.result["has_more"], false);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_read_unknown_id_errors() {
    let (server_task, client, _state, _tmp) = spawn_server_with_session_manager().await;

    client
        .send(req(
            1,
            "session/read",
            serde_json::json!({ "session_id": "does-not-exist" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("session/read"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_resume_installs_session_from_disk() {
    let (server_task, client, state, _tmp) = spawn_server_with_session_manager().await;

    let manager = state.session_manager.read().await.clone().unwrap();
    manager
        .save(&coco_session::Session {
            id: "resume-test".into(),
            created_at: "100".into(),
            updated_at: None,
            model: "claude-opus-4-6".into(),
            working_dir: std::path::PathBuf::from("/tmp/resume"),
            title: None,
            message_count: 0,
            total_tokens: 0,
        })
        .unwrap();

    client
        .send(req(
            1,
            "session/resume",
            serde_json::json!({ "session_id": "resume-test" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["session"]["session_id"], "resume-test");
            assert_eq!(r.result["session"]["model"], "claude-opus-4-6");
        }
        other => panic!("expected Response, got {other:?}"),
    }

    // Verify the session is now the active SDK session.
    let slot = state.session.read().await;
    let session = slot.as_ref().expect("session should be installed");
    assert_eq!(session.session_id, "resume-test");
    assert_eq!(session.model, "claude-opus-4-6");
    assert_eq!(session.cwd, "/tmp/resume");

    drop(slot);
    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_resume_unknown_id_errors() {
    let (server_task, client, _state, _tmp) = spawn_server_with_session_manager().await;

    client
        .send(req(
            1,
            "session/resume",
            serde_json::json!({ "session_id": "nope" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("session/resume"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn session_archive_deletes_persisted_session() {
    let (server_task, client, state, _tmp) = spawn_server_with_session_manager().await;

    // Start + archive — verify the disk record is removed.
    client
        .send(req(1, "session/start", serde_json::json!({})))
        .await
        .unwrap();
    let start_reply = client.recv().await.unwrap().unwrap();
    let session_id = match start_reply {
        JsonRpcMessage::Response(r) => r.result["session_id"].as_str().unwrap().to_string(),
        other => panic!("expected Response, got {other:?}"),
    };

    // Confirm it was persisted
    let manager = state.session_manager.read().await.clone().unwrap();
    assert!(manager.load(&session_id).is_ok());

    // Archive
    client
        .send(req(
            2,
            "session/archive",
            serde_json::json!({ "session_id": session_id.clone() }),
        ))
        .await
        .unwrap();
    // Drain notification + response in either order.
    for _ in 0..2 {
        let _ = client.recv().await.unwrap().unwrap();
    }

    // Verify the persisted record is gone.
    assert!(
        manager.load(&session_id).is_err(),
        "session/archive should delete the persisted record"
    );

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.12: config/read + config/value/write ----------------

/// Scoped temp directory used as a fake project cwd. Creates
/// `.claude/` on construction; cleans up on drop.
struct TempProjectDir {
    path: std::path::PathBuf,
}

impl TempProjectDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("coco-sdk-test-project-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(path.join(".claude")).unwrap();
        Self { path }
    }
}

impl Drop for TempProjectDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Start a session pointing at the given cwd. Used by config tests so
/// config/read + config/write resolve paths relative to the tempdir
/// instead of the real process cwd.
async fn start_session_with_cwd(client: &InMemoryTransport, cwd: &std::path::Path) {
    client
        .send(req(
            1,
            "session/start",
            serde_json::json!({ "cwd": cwd.to_string_lossy() }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();
}

#[tokio::test]
async fn config_read_returns_merged_settings_from_project_scope() {
    let tmp = TempProjectDir::new();
    // Pre-populate a project settings file.
    let project_settings = tmp.path.join(".claude/settings.json");
    std::fs::write(&project_settings, r#"{"auto_updater_status":"enabled"}"#).unwrap();

    let (server_task, client) = spawn_server().await;
    start_session_with_cwd(&client, &tmp.path).await;

    client
        .send(req(2, "config/read", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            // The merged config should contain the project setting we wrote.
            // We don't assert on the full config because user-scope settings
            // outside our control may also be merged in from ~/.coco/.
            let sources = r.result["sources"].as_object().unwrap();
            assert!(
                sources.contains_key("project"),
                "per-source map should include 'project' since we wrote a project settings file"
            );
            assert_eq!(
                sources["project"]["auto_updater_status"], "enabled",
                "project source should round-trip our written value"
            );
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_write_project_scope_persists_to_disk() {
    let tmp = TempProjectDir::new();
    let (server_task, client) = spawn_server().await;
    start_session_with_cwd(&client, &tmp.path).await;

    // Write a setting at project scope.
    client
        .send(req(
            2,
            "config/value/write",
            serde_json::json!({
                "key": "auto_updater_status",
                "value": "disabled",
                "scope": "project",
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => assert!(r.result.is_null()),
        other => panic!("expected Response, got {other:?}"),
    }

    // Verify the file on disk.
    let settings_path = tmp.path.join(".claude/settings.json");
    let contents = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed["auto_updater_status"], "disabled");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_write_local_scope_persists_to_separate_file() {
    let tmp = TempProjectDir::new();
    let (server_task, client) = spawn_server().await;
    start_session_with_cwd(&client, &tmp.path).await;

    client
        .send(req(
            2,
            "config/value/write",
            serde_json::json!({
                "key": "theme",
                "value": "dark",
                "scope": "local",
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    // Local scope goes to .claude/settings.local.json, NOT settings.json.
    let local_path = tmp.path.join(".claude/settings.local.json");
    let project_path = tmp.path.join(".claude/settings.json");
    assert!(
        local_path.exists(),
        "local scope should create settings.local.json"
    );
    assert!(
        !project_path.exists(),
        "local scope must not write to project settings.json"
    );
    let contents = std::fs::read_to_string(&local_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed["theme"], "dark");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_write_invalid_scope_errors() {
    let (server_task, client) = spawn_server().await;
    start_session(&client).await;

    client
        .send(req(
            2,
            "config/value/write",
            serde_json::json!({
                "key": "foo",
                "value": "bar",
                "scope": "bogus",
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_PARAMS);
            assert!(e.message.contains("invalid scope"));
            assert!(e.message.contains("bogus"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_write_nested_key_creates_intermediate_objects() {
    let tmp = TempProjectDir::new();
    let (server_task, client) = spawn_server().await;
    start_session_with_cwd(&client, &tmp.path).await;

    // Write a nested dotted key. Intermediate "permissions" object
    // must be created automatically.
    client
        .send(req(
            2,
            "config/value/write",
            serde_json::json!({
                "key": "permissions.default_mode",
                "value": "plan",
                "scope": "project",
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    // Verify the nested structure was created.
    let contents = std::fs::read_to_string(tmp.path.join(".claude/settings.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(
        parsed["permissions"]["default_mode"], "plan",
        "nested dotted key should write to nested JSON object"
    );

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_write_then_read_roundtrip() {
    // Write a value at project scope, then read it back via
    // config/read and verify it appears in the per-source map.
    let tmp = TempProjectDir::new();
    let (server_task, client) = spawn_server().await;
    start_session_with_cwd(&client, &tmp.path).await;

    client
        .send(req(
            2,
            "config/value/write",
            serde_json::json!({
                "key": "auto_updater_status",
                "value": "disabled",
                "scope": "project",
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    client
        .send(req(3, "config/read", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(
                r.result["sources"]["project"]["auto_updater_status"],
                "disabled"
            );
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.13: batched stubs + observability -------------------

#[tokio::test]
async fn mcp_status_returns_empty_list_when_no_manager_wired() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "mcp/status", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["mcpServers"].as_array().unwrap().len(), 0);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn plugin_reload_returns_empty_result() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "plugin/reload", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["plugins"].as_array().unwrap().len(), 0);
            assert_eq!(r.result["commands"].as_array().unwrap().len(), 0);
            assert_eq!(r.result["agents"].as_array().unwrap().len(), 0);
            assert_eq!(r.result["error_count"], 0);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn config_apply_flags_acknowledges_flags() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            1,
            "config/applyFlags",
            serde_json::json!({
                "settings": {
                    "feature_x": true,
                    "rollout_pct": 50,
                }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert!(r.result.is_null());
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn context_usage_errors_without_active_session() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(1, "context/usage", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no active session"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn context_usage_reports_accumulated_session_stats() {
    // Install a StatsEmittingRunner so we can fold synthetic stats
    // into session.stats, then call context/usage and verify the
    // totals match.
    let runner = Arc::new(StatsEmittingRunner {
        cost_usd: 0.03,
        input_tokens: 3000,
        output_tokens: 1500,
        stop_reason: "end_turn".into(),
    });
    let (server_task, client) = spawn_server_with_runner(runner).await;

    start_session(&client).await;

    // Fire a turn so stats accumulate.
    client
        .send(req(2, "turn/start", serde_json::json!({ "prompt": "go" })))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Query context/usage.
    client
        .send(req(3, "context/usage", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["total_tokens"], 4500);
            assert_eq!(r.result["max_tokens"], 200000);
            assert_eq!(r.result["model"], "claude-opus-4-6");
            assert_eq!(r.result["is_auto_compact_enabled"], true);
            // 4500 / 200000 = 2.25%
            let pct = r.result["percentage"].as_f64().unwrap();
            assert!((pct - 2.25).abs() < 0.01);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn hook_callback_response_delivers_output_to_pending_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_hook_callback("cb-1".into()).await;

    client
        .send(req(
            1,
            "hook/callbackResponse",
            serde_json::json!({
                "callback_id": "cb-1",
                "output": { "behavior": "allow", "stdout": "ok" }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert_eq!(params.callback_id, "cb-1");
    assert_eq!(params.output["behavior"], "allow");
    assert_eq!(params.output["stdout"], "ok");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn hook_callback_response_unknown_id_errors() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "hook/callbackResponse",
            serde_json::json!({
                "callback_id": "missing",
                "output": {}
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no pending hook callback"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_route_message_response_delivers_to_pending_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_mcp_route("rid-42".into()).await;

    client
        .send(req(
            1,
            "mcp/routeMessageResponse",
            serde_json::json!({
                "request_id": "rid-42",
                "message": { "result": { "content": "ok" } }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert_eq!(params.request_id, "rid-42");
    assert_eq!(params.message["result"]["content"], "ok");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_route_message_response_unknown_id_errors() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "mcp/routeMessageResponse",
            serde_json::json!({
                "request_id": "never-was",
                "message": {}
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no pending mcp route"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn elicitation_resolve_delivers_values_to_pending_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_elicitation("elc-1".into()).await;

    client
        .send(req(
            1,
            "elicitation/resolve",
            serde_json::json!({
                "request_id": "elc-1",
                "mcp_server_name": "github",
                "approved": true,
                "values": { "token": "abc123", "org": "anthropic" }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert_eq!(params.request_id, "elc-1");
    assert_eq!(params.mcp_server_name, "github");
    assert!(params.approved);
    assert_eq!(params.values.get("token").unwrap(), "abc123");
    assert_eq!(params.values.get("org").unwrap(), "anthropic");

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn elicitation_resolve_rejected_delivers_to_pending_receiver() {
    let (server_task, client, state) = spawn_server_with_state().await;

    let rx = state.register_elicitation("elc-2".into()).await;

    client
        .send(req(
            1,
            "elicitation/resolve",
            serde_json::json!({
                "request_id": "elc-2",
                "mcp_server_name": "github",
                "approved": false
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    let params = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("receiver should be awoken")
        .expect("oneshot should deliver");
    assert!(!params.approved);
    assert!(params.values.is_empty());

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.14b: control/rewindFiles ----------------------------

#[tokio::test]
async fn rewind_files_errors_without_active_session() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            1,
            "control/rewindFiles",
            serde_json::json!({
                "user_message_id": "msg-1",
                "dry_run": true
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no active session"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn rewind_files_errors_when_file_history_not_enabled() {
    let (server_task, client) = spawn_server().await;
    start_session(&client).await;

    client
        .send(req(
            2,
            "control/rewindFiles",
            serde_json::json!({
                "user_message_id": "msg-1",
                "dry_run": true
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("file history not enabled"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

// ----- Phase 2.C.14c: MCP lifecycle ----------------------------------

/// Spawn a server with an empty MCP manager pointing at a tempdir.
async fn spawn_server_with_mcp_manager() -> (
    tokio::task::JoinHandle<()>,
    Arc<InMemoryTransport>,
    Arc<SdkServerState>,
    TempSessionsDir,
) {
    let tmp = TempSessionsDir::new();
    let manager = Arc::new(tokio::sync::Mutex::new(
        coco_mcp::McpConnectionManager::new(tmp.path.clone()),
    ));
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end).with_mcp_manager(manager);
    let state = server.state();
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (handle, client_end, state, tmp)
}

#[tokio::test]
async fn mcp_set_servers_without_manager_errors() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            1,
            "mcp/setServers",
            serde_json::json!({ "servers": {} }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("MCP manager not enabled"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_set_servers_registers_stdio_config() {
    let (server_task, client, state, _tmp) = spawn_server_with_mcp_manager().await;

    client
        .send(req(
            1,
            "mcp/setServers",
            serde_json::json!({
                "servers": {
                    "github": {
                        "transport": "stdio",
                        "command": "/usr/bin/echo",
                        "args": ["hello"]
                    }
                }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            let added = r.result["added"].as_array().unwrap();
            assert_eq!(added.len(), 1);
            assert_eq!(added[0], "github");
            // No errors.
            assert_eq!(r.result["errors"].as_object().unwrap().len(), 0);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    // Verify the manager has the server registered.
    let manager = state.mcp_manager.read().await.clone().unwrap();
    let manager = manager.lock().await;
    let names = manager.registered_server_names();
    assert!(names.contains(&"github".to_string()));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_set_servers_reports_invalid_config_in_errors() {
    let (server_task, client, _state, _tmp) = spawn_server_with_mcp_manager().await;

    client
        .send(req(
            1,
            "mcp/setServers",
            serde_json::json!({
                "servers": {
                    "broken": {
                        "transport": "stdio"
                        // missing required `command` field
                    }
                }
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.result["added"].as_array().unwrap().len(), 0);
            let errors = r.result["errors"].as_object().unwrap();
            assert!(errors.contains_key("broken"));
            let msg = errors["broken"].as_str().unwrap();
            assert!(msg.contains("invalid mcp config"));
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_status_lists_registered_servers_after_set_servers() {
    // After mcp/setServers registers a server, mcp/status should
    // report it (in "disconnected" state since we never call connect).
    let (server_task, client, _state, _tmp) = spawn_server_with_mcp_manager().await;

    client
        .send(req(
            1,
            "mcp/setServers",
            serde_json::json!({
                "servers": {
                    "github": {
                        "transport": "stdio",
                        "command": "/usr/bin/echo"
                    }
                }
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    client
        .send(req(2, "mcp/status", serde_json::json!({})))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Response(r) => {
            let servers = r.result["mcpServers"].as_array().unwrap();
            assert_eq!(servers.len(), 1);
            assert_eq!(servers[0]["name"], "github");
            // Not connected — the registered config exists but no
            // connect was attempted.
            assert_eq!(servers[0]["status"], "disconnected");
            assert_eq!(servers[0]["tool_count"], 0);
        }
        other => panic!("expected Response, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_toggle_disable_is_no_op_on_unconnected_server() {
    // Toggle a not-yet-connected server to disabled. This should
    // succeed (disconnect on a missing server is a no-op in the
    // manager).
    let (server_task, client, _state, _tmp) = spawn_server_with_mcp_manager().await;

    client
        .send(req(
            1,
            "mcp/setServers",
            serde_json::json!({
                "servers": {
                    "github": {
                        "transport": "stdio",
                        "command": "/usr/bin/echo"
                    }
                }
            }),
        ))
        .await
        .unwrap();
    let _ = client.recv().await.unwrap().unwrap();

    client
        .send(req(
            2,
            "mcp/toggle",
            serde_json::json!({
                "server_name": "github",
                "enabled": false
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    assert!(matches!(reply, JsonRpcMessage::Response(_)));

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_reconnect_without_manager_errors() {
    let (server_task, client) = spawn_server().await;

    client
        .send(req(
            1,
            "mcp/reconnect",
            serde_json::json!({ "server_name": "github" }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("MCP manager not enabled"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}

#[tokio::test]
async fn rewind_files_errors_on_unknown_message_id() {
    // Wire an empty FileHistoryState — no snapshots exist.
    let history = Arc::new(tokio::sync::RwLock::new(
        coco_context::FileHistoryState::new(),
    ));
    let tmp_config = TempSessionsDir::new(); // reuse temp helper for a tmpdir
    let (server_end, client_end) = InMemoryTransport::pair(32);
    let server = SdkServer::new(server_end).with_file_history(history, tmp_config.path.clone());
    let server_task = tokio::spawn(async move {
        let _ = server.run().await;
    });

    start_session(&client_end).await;

    client_end
        .send(req(
            2,
            "control/rewindFiles",
            serde_json::json!({
                "user_message_id": "never-snapshotted",
                "dry_run": true
            }),
        ))
        .await
        .unwrap();
    let reply = client_end.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no snapshot for user_message_id"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client_end);
    server_task.await.unwrap();
}

#[tokio::test]
async fn elicitation_resolve_unknown_id_errors() {
    let (server_task, client, _state) = spawn_server_with_state().await;

    client
        .send(req(
            1,
            "elicitation/resolve",
            serde_json::json!({
                "request_id": "never-was",
                "mcp_server_name": "github",
                "approved": true
            }),
        ))
        .await
        .unwrap();
    let reply = client.recv().await.unwrap().unwrap();
    match reply {
        JsonRpcMessage::Error(e) => {
            assert_eq!(e.code, error_codes::INVALID_REQUEST);
            assert!(e.message.contains("no pending elicitation"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(client);
    server_task.await.unwrap();
}
