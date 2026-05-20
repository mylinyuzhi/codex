//! Mock-only end-to-end coverage for the agent-team TS mirror.
//!
//! These tests intentionally live under `coco-rs/tests/live` even
//! though they do not call a live provider: the crate is the workspace's
//! cross-crate integration-test home. The suite exercises the public
//! coordinator runner + mailbox API with real on-disk inbox files, so it
//! catches wiring regressions that crate-local unit tests can miss.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;
use coco_coordinator::constants::TEAM_LEAD_NAME;
use coco_coordinator::mailbox;
use coco_coordinator::pane::SystemPromptMode;
use coco_coordinator::runner_loop::AgentExecutionEngine;
use coco_coordinator::runner_loop::AgentQueryConfig;
use coco_coordinator::runner_loop::AgentQueryResult;
use coco_coordinator::runner_loop::InProcessRunnerConfig;
use coco_coordinator::runner_loop::run_in_process_teammate;
use coco_coordinator::task::InProcessTeammateTaskState;
use coco_coordinator::types::TeammateIdentity;
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::oneshot;

struct HomeGuard {
    previous: Option<String>,
    _tmp: TempDir,
}

impl HomeGuard {
    fn install() -> Self {
        let tmp = tempfile::tempdir().expect("temp home");
        let previous = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        Self {
            previous,
            _tmp: tmp,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

struct LiveControlEngine {
    started: Mutex<Option<oneshot::Sender<()>>>,
    observed: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl AgentExecutionEngine for LiveControlEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        config: AgentQueryConfig,
    ) -> coco_coordinator::Result<AgentQueryResult> {
        if let Some(tx) = self.started.lock().await.take() {
            let _ = tx.send(());
        }

        let live_mode = config
            .live_permission_mode
            .expect("runner must pass live permission mode");
        let live_rules = config
            .live_permission_rules
            .expect("runner must pass live permission rules");
        let cancel = config.cancel.expect("runner must pass per-turn cancel");

        loop {
            let mode_seen = *live_mode.read().await == coco_types::PermissionMode::AcceptEdits;
            let rule_seen = live_rules.read().await.iter().any(|rule| {
                rule.behavior == coco_types::PermissionBehavior::Allow
                    && rule.value.tool_pattern == "Edit"
                    && rule.value.rule_content.as_deref() == Some("/repo/**")
            });

            if mode_seen && rule_seen {
                if let Some(tx) = self.observed.lock().await.take() {
                    let _ = tx.send(());
                }
                return Ok(AgentQueryResult {
                    messages: Vec::new(),
                    token_count: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    turns: 1,
                    tool_use_count: 0,
                    cancelled: false,
                    response_text: Some("observed live control".into()),
                });
            }

            tokio::select! {
                _ = cancel.cancelled() => {
                    return Ok(cancelled_turn_result());
                }
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }
}

struct InterruptibleEngine {
    started: Mutex<Option<oneshot::Sender<()>>>,
    interrupted: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl AgentExecutionEngine for InterruptibleEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        config: AgentQueryConfig,
    ) -> coco_coordinator::Result<AgentQueryResult> {
        if let Some(tx) = self.started.lock().await.take() {
            let _ = tx.send(());
        }
        let cancel = config.cancel.expect("runner must pass per-turn cancel");
        cancel.cancelled().await;
        if let Some(tx) = self.interrupted.lock().await.take() {
            let _ = tx.send(());
        }
        Ok(cancelled_turn_result())
    }
}

fn cancelled_turn_result() -> AgentQueryResult {
    AgentQueryResult {
        messages: Vec::new(),
        token_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        turns: 1,
        tool_use_count: 0,
        cancelled: true,
        response_text: None,
    }
}

fn successful_turn_result(text: &str) -> AgentQueryResult {
    AgentQueryResult {
        messages: vec![std::sync::Arc::new(
            coco_messages::create_assistant_message(
                vec![coco_messages::AssistantContent::Text(
                    coco_messages::TextContent {
                        text: text.into(),
                        provider_metadata: None,
                    },
                )],
                "test-model",
                coco_types::TokenUsage::default(),
            ),
        )],
        token_count: 10,
        input_tokens: 6,
        output_tokens: 4,
        turns: 1,
        tool_use_count: 0,
        cancelled: false,
        response_text: Some(text.into()),
    }
}

struct OneShotEngine;

#[async_trait]
impl AgentExecutionEngine for OneShotEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        config: AgentQueryConfig,
    ) -> coco_coordinator::Result<AgentQueryResult> {
        assert_eq!(
            config.permission_mode.as_deref(),
            Some("plan"),
            "plan-mode-required teammates must start their first turn in plan mode"
        );
        Ok(successful_turn_result("plan written"))
    }
}

fn identity(team_name: &str) -> TeammateIdentity {
    TeammateIdentity {
        agent_id: format!("worker@{team_name}"),
        agent_name: "worker".into(),
        team_name: team_name.into(),
        color: None,
        plan_mode_required: false,
    }
}

fn task_state(team_name: &str) -> Arc<RwLock<InProcessTeammateTaskState>> {
    Arc::new(RwLock::new(InProcessTeammateTaskState::new(
        format!("task-worker@{team_name}"),
        identity(team_name),
        "initial".into(),
    )))
}

fn runner_config(team_name: &str, cancelled: Arc<AtomicBool>) -> InProcessRunnerConfig {
    InProcessRunnerConfig {
        identity: identity(team_name),
        task_id: format!("task-worker@{team_name}"),
        prompt: "Do the task".into(),
        model: None,
        system_prompt: None,
        system_prompt_mode: SystemPromptMode::Default,
        allowed_tools: Vec::new(),
        allow_permission_prompts: false,
        max_turns: None,
        cancelled,
        auto_compact_threshold: 100_000,
        bypass_permissions_available: false,
        features: None,
        tool_overrides: None,
        parent_tool_filter: None,
        effort: None,
        use_exact_tools: false,
        mcp_servers: Vec::new(),
        disallowed_tools: Vec::new(),
        model_role: None,
        model_selection: coco_types::LlmModelSelection::InheritMain,
        task_list: None,
        roster_store: None,
        plan_mode_required: false,
        hooks: None,
        orchestration_ctx: None,
    }
}

fn write_mode_set(team_name: &str) {
    mailbox::write_to_mailbox(
        "worker",
        mailbox::TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text: mailbox::create_mode_set_request(
                coco_types::PermissionMode::AcceptEdits,
                TEAM_LEAD_NAME,
            ),
            timestamp: "now".into(),
            read: false,
            color: None,
            summary: Some("mode".into()),
        },
        team_name,
    )
    .expect("write mode_set_request");
}

fn write_team_permission_update(team_name: &str) {
    let text = serde_json::to_string(&mailbox::ProtocolMessage::TeamPermissionUpdate {
        permission_update: mailbox::protocol::WireTeamPermissionUpdate::AddRules {
            rules: vec![mailbox::protocol::WirePermissionRuleValue {
                tool_name: "Edit".into(),
                rule_content: Some("/repo/**".into()),
            }],
            behavior: coco_types::PermissionBehavior::Allow,
            destination: mailbox::protocol::WireTeamPermissionUpdateDestination::Session,
        },
        directory_path: "/repo".into(),
        tool_name: "Edit".into(),
    })
    .expect("serialize team permission update");

    mailbox::write_to_mailbox(
        "worker",
        mailbox::TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text,
            timestamp: "now".into(),
            read: false,
            color: None,
            summary: Some("permission".into()),
        },
        team_name,
    )
    .expect("write team_permission_update");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agentteam_mirror_e2e_suite() {
    let _home = HomeGuard::install();

    protocol_boundary_rejects_non_ts_shapes();
    active_query_drains_mailbox_control_into_live_query_state().await;
    current_work_interrupt_cancels_turn_without_killing_lifecycle().await;
    plan_mode_required_first_turn_uses_plan_mode().await;
}

fn protocol_boundary_rejects_non_ts_shapes() {
    let legacy_team_update = r#"{"type":"team_permission_update","permission_update":{"type":"add_rules","rules":[{"tool_name":"Edit","rule_content":"/repo/**"}],"behavior":"allow","destination":"session"},"directory_path":"/repo","tool_name":"Edit"}"#;
    assert!(
        mailbox::parse_protocol_message(legacy_team_update).is_none(),
        "mailbox must reject pre-refactor snake_case team permission updates"
    );

    let bad_mode = r#"{"type":"mode_set_request","mode":"not-a-mode","from":"team-lead"}"#;
    assert!(
        mailbox::parse_protocol_message(bad_mode).is_none(),
        "mode_set_request.mode must stay typed as PermissionMode"
    );

    let bad_permission_response =
        r#"{"type":"permission_response","request_id":"r1","subtype":"maybe"}"#;
    assert!(
        mailbox::parse_protocol_message(bad_permission_response).is_none(),
        "permission_response.subtype must stay a success/error union"
    );
}

async fn active_query_drains_mailbox_control_into_live_query_state() {
    let team_name = format!("mirror-live-{}", uuid::Uuid::new_v4().simple());
    let cancelled = Arc::new(AtomicBool::new(false));
    let config = runner_config(&team_name, cancelled.clone());
    let state = task_state(&team_name);
    let (started_tx, started_rx) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let engine = Arc::new(LiveControlEngine {
        started: Mutex::new(Some(started_tx)),
        observed: Mutex::new(Some(observed_tx)),
    });

    let run = {
        let state = state.clone();
        let engine = engine.clone();
        tokio::spawn(async move { run_in_process_teammate(config, engine.as_ref(), &state).await })
    };

    started_rx.await.expect("engine must start");

    let writer_stop = tokio_util::sync::CancellationToken::new();
    let writer = {
        let team_name = team_name.clone();
        let writer_stop = writer_stop.clone();
        tokio::spawn(async move {
            loop {
                write_mode_set(&team_name);
                write_team_permission_update(&team_name);
                tokio::select! {
                    _ = writer_stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
        })
    };

    tokio::time::timeout(Duration::from_secs(3), observed_rx)
        .await
        .expect("active query should observe live mailbox control updates")
        .expect("engine should signal observation");
    writer_stop.cancel();
    writer.await.expect("writer stops");

    cancelled.store(true, Ordering::Relaxed);
    let result = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("runner exits after lifecycle cancel")
        .expect("runner task joins");
    assert!(result.success);
}

async fn current_work_interrupt_cancels_turn_without_killing_lifecycle() {
    let team_name = format!("mirror-interrupt-{}", uuid::Uuid::new_v4().simple());
    let cancelled = Arc::new(AtomicBool::new(false));
    let config = runner_config(&team_name, cancelled.clone());
    let state = task_state(&team_name);
    let (started_tx, started_rx) = oneshot::channel();
    let (interrupted_tx, interrupted_rx) = oneshot::channel();
    let engine = Arc::new(InterruptibleEngine {
        started: Mutex::new(Some(started_tx)),
        interrupted: Mutex::new(Some(interrupted_tx)),
    });

    let run = {
        let state = state.clone();
        let engine = engine.clone();
        tokio::spawn(async move { run_in_process_teammate(config, engine.as_ref(), &state).await })
    };

    started_rx.await.expect("engine must start");
    assert!(
        state.read().await.interrupt_current_work(),
        "current work token must be exposed while query is active"
    );
    interrupted_rx
        .await
        .expect("engine must observe per-turn interrupt");

    for _ in 0..50 {
        if state.read().await.is_idle {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let snapshot = state.read().await;
    assert!(snapshot.is_idle, "teammate should return to idle");
    assert!(
        snapshot.current_work_cancel.is_none(),
        "per-turn token should be cleared after interrupt"
    );
    assert!(
        snapshot
            .messages
            .iter()
            .any(|message| message.content == "Interrupted by user."),
        "interrupt should be mirrored into teammate transcript"
    );
    drop(snapshot);

    cancelled.store(true, Ordering::Relaxed);
    let result = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("runner exits after lifecycle cancel")
        .expect("runner task joins");
    assert!(result.success);
    assert_eq!(result.turns, 1);
}

async fn plan_mode_required_first_turn_uses_plan_mode() {
    let team_name = format!("mirror-plan-{}", uuid::Uuid::new_v4().simple());
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut config = runner_config(&team_name, cancelled.clone());
    config.plan_mode_required = true;
    let state = task_state(&team_name);

    let responder_stop = tokio_util::sync::CancellationToken::new();
    let responder = {
        let team_name = team_name.clone();
        let responder_stop = responder_stop.clone();
        tokio::spawn(async move {
            loop {
                let messages = mailbox::read_mailbox(TEAM_LEAD_NAME, &team_name).unwrap();
                if let Some((idx, request_id)) =
                    messages.iter().enumerate().find_map(|(idx, message)| {
                        match mailbox::parse_protocol_message(&message.text) {
                            Some(mailbox::ProtocolMessage::PlanApprovalRequest {
                                request_id,
                                ..
                            }) => Some((idx, request_id)),
                            _ => None,
                        }
                    })
                {
                    let response =
                        serde_json::to_string(&mailbox::ProtocolMessage::PlanApprovalResponse {
                            request_id,
                            approved: true,
                            feedback: None,
                            timestamp: "now".into(),
                            permission_mode: Some("plan".into()),
                        })
                        .unwrap();
                    mailbox::write_to_mailbox(
                        "worker",
                        mailbox::TeammateMessage {
                            from: TEAM_LEAD_NAME.into(),
                            text: response,
                            timestamp: "now".into(),
                            read: false,
                            color: None,
                            summary: Some("plan approved".into()),
                        },
                        &team_name,
                    )
                    .unwrap();
                    let _ = mailbox::mark_message_as_read_by_index(TEAM_LEAD_NAME, &team_name, idx);
                    break;
                }
                tokio::select! {
                    _ = responder_stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
            }
        })
    };

    let engine = Arc::new(OneShotEngine);
    let run = {
        let state = state.clone();
        let engine = engine.clone();
        tokio::spawn(async move { run_in_process_teammate(config, engine.as_ref(), &state).await })
    };

    for _ in 0..50 {
        if state.read().await.is_idle {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    responder_stop.cancel();
    responder.await.expect("plan responder joins");

    assert_eq!(
        state.read().await.permission_mode,
        coco_types::PermissionMode::Plan
    );

    cancelled.store(true, Ordering::Relaxed);
    let result = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("plan-mode runner exits after lifecycle cancel")
        .expect("plan-mode runner joins");
    assert!(result.success);
}
