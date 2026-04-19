//! Tests for per-turn plan-mode reminder injection.

use super::PlanModeReminder;
use coco_messages::MessageHistory;
use coco_types::Message;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use coco_types::UserContent;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;

fn text_of(msg: &Message) -> Option<String> {
    match msg {
        Message::Attachment(a) => match &a.message {
            coco_types::LlmMessage::User { content, .. } => content.iter().find_map(|c| match c {
                UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn history_texts(history: &MessageHistory) -> Vec<String> {
    history.messages.iter().filter_map(text_of).collect()
}

#[tokio::test]
async fn no_injection_when_not_in_plan_mode() {
    let mut r = PlanModeReminder::new(PermissionMode::Default, Some("s1".into()), None, None, None);
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    assert!(h.messages.is_empty());
}

#[tokio::test]
async fn first_turn_in_plan_emits_full_reminder() {
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        None,
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    let t = &texts[0];
    assert!(t.contains("<system-reminder>"));
    assert!(t.contains("Plan mode is active"));
    assert!(t.contains("Plan File Info"));
    assert!(t.contains("## Plan Workflow"));
}

#[tokio::test]
async fn subsequent_turns_throttle_then_emit_sparse_reminder() {
    // TS parity: first turn emits Full, then 4 human turns are skipped
    // (TURNS_BETWEEN_ATTACHMENTS = 5), then human turn 6 emits Sparse.
    // `turn_start` bumps the throttle counter only when it observes a
    // NEW human-turn UUID in history — so tests must push a fresh user
    // message between each call to simulate a new human turn.
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state),
    );
    let mut h = MessageHistory::new();

    // Human turn 1: Full
    h.messages
        .push(coco_messages::create_user_message("turn 1"));
    r.turn_start(&mut h).await;
    assert_eq!(history_texts(&h).len(), 1, "turn 1 emits");

    // Human turns 2-5: throttled (each gets a fresh user message).
    for i in 2..=5 {
        h.messages
            .push(coco_messages::create_user_message(&format!("turn {i}")));
        r.turn_start(&mut h).await;
    }
    assert_eq!(history_texts(&h).len(), 1, "turns 2-5 should be throttled");

    // Repeating `turn_start` on turn 5 (tool-result round, same user
    // message UUID) must NOT advance the counter — TS parity: only
    // human turns count. If we broke this, tool rounds would prematurely
    // trigger the next reminder.
    r.turn_start(&mut h).await;
    assert_eq!(
        history_texts(&h).len(),
        1,
        "tool-round re-entries on the same human turn do not advance cadence"
    );

    // Human turn 6: Sparse fires (5 human turns since last attachment).
    h.messages
        .push(coco_messages::create_user_message("turn 6"));
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 2, "turn 6 emits second reminder");
    assert!(texts[0].contains("## Plan Workflow"), "first is Full");
    assert!(
        texts[1].contains("Plan mode still active"),
        "second is Sparse"
    );
}

#[tokio::test]
async fn full_reminder_cycles_every_n_attachments() {
    // Attachments 1, 6, 11 should be Full; others Sparse. We approximate
    // by pre-seeding the counter on app_state so we don't need to run 25
    // turns with the throttle delays.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        plan_mode_attachment_count: 5, // next attachment will be #6 → Full
        plan_mode_turns_since_last_attachment: 10, // past the throttle
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(
        texts[0].contains("## Plan Workflow"),
        "attachment #6 is Full: {}",
        texts[0]
    );
}

#[tokio::test]
async fn sub_agent_reminder_uses_subagent_variant() {
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        Some("aabcdef0".into()),
        Some(tmp.path().to_path_buf()),
        None,
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    // Sub-agent variant still restricts writes but skips the 5-phase
    // workflow text (the parent agent drives workflow).
    let t = &texts[0];
    assert!(t.contains("Plan mode is active"));
    assert!(!t.contains("## Plan Workflow"));
    assert!(t.contains("Plan File Info"));
}

#[tokio::test]
async fn exit_reminder_fires_once_when_flag_set() {
    let app_state = Arc::new(RwLock::new(ToolAppState {
        needs_plan_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    // Engine is back to Default (tool restored mode), flag is still set
    // from the trailing tool — exit reminder should still fire.
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("Exited Plan Mode"));

    // Second turn: flag was cleared, no more injections.
    let mut h2 = MessageHistory::new();
    r.turn_start(&mut h2).await;
    assert!(h2.messages.is_empty());

    // And the flag on app_state is now false.
    let guard = app_state.read().await;
    assert!(!guard.needs_plan_mode_exit_attachment);
}

#[tokio::test]
async fn exit_and_plan_reminder_can_coexist_in_same_turn() {
    // Edge case: engine is still marked Plan (stale), and a prior
    // tool set the exit flag. Ordering: exit first, plan second.
    // This matches TS where the exit reminder is a one-shot trailing
    // signal and plan mode still injects its steady-state reminder.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        needs_plan_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains("Exited Plan Mode"));
    assert!(texts[1].contains("Plan mode is active"));
}

#[tokio::test]
async fn exit_flag_false_emits_nothing() {
    // Default struct already has `needs_plan_mode_exit_attachment: false`;
    // verify the reminder stays silent.
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        None,
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    assert!(h.messages.is_empty());
}

#[tokio::test]
async fn plan_reminder_reports_existing_plan_file() {
    let tmp = tempdir().unwrap();
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    let session_id = "test-existing-plan";
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    // PlanModeReminder takes the plans dir directly (not the config
    // home) to stay independent of settings lookup at injection time.
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some(session_id.to_string()),
        None,
        Some(plans_dir),
        None,
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(
        texts[0].contains("A plan file already exists"),
        "Full reminder must detect existing plan file; got: {}",
        texts[0]
    );
}

// ── Teammate approval polling (F3) ──

/// In-memory mailbox handle so tests can inject messages + verify reads.
#[derive(Default)]
struct FakeMailbox {
    inboxes:
        std::sync::Mutex<std::collections::HashMap<(String, String), Vec<coco_tool::InboxMessage>>>,
    marked: std::sync::Mutex<Vec<(String, String, usize)>>,
    // Tracks per-inbox next-index for synthetic messages.
    next_index: std::sync::Mutex<std::collections::HashMap<(String, String), usize>>,
}

impl FakeMailbox {
    fn push(&self, agent: &str, team: &str, text: &str, from: &str) {
        let key = (agent.to_string(), team.to_string());
        let mut idx_guard = self.next_index.lock().unwrap();
        let idx = idx_guard.entry(key.clone()).or_insert(0);
        let this_idx = *idx;
        *idx += 1;
        drop(idx_guard);
        self.inboxes
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .push(coco_tool::InboxMessage {
                index: this_idx,
                from: from.into(),
                text: text.into(),
                timestamp: "t".into(),
            });
    }
}

#[async_trait::async_trait]
impl coco_tool::MailboxHandle for FakeMailbox {
    async fn write_to_mailbox(
        &self,
        _recipient: &str,
        _team: &str,
        _m: coco_tool::MailboxEnvelope,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn read_unread(
        &self,
        agent: &str,
        team: &str,
    ) -> anyhow::Result<Vec<coco_tool::InboxMessage>> {
        Ok(self
            .inboxes
            .lock()
            .unwrap()
            .get(&(agent.to_string(), team.to_string()))
            .cloned()
            .unwrap_or_default())
    }
    async fn mark_read(&self, agent: &str, team: &str, index: usize) -> anyhow::Result<()> {
        self.marked
            .lock()
            .unwrap()
            .push((agent.into(), team.into(), index));
        // Also remove it so a subsequent read doesn't re-surface.
        let mut guard = self.inboxes.lock().unwrap();
        if let Some(msgs) = guard.get_mut(&(agent.to_string(), team.to_string())) {
            msgs.retain(|m| m.index != index);
        }
        Ok(())
    }
}

#[tokio::test]
async fn teammate_polls_approval_and_injects_approved_reminder() {
    let app_state = Arc::new(RwLock::new(ToolAppState {
        awaiting_plan_approval_request_id: Some("plan_approval-alice-team-a-deadbeef".into()),
        awaiting_plan_approval: true,
        ..Default::default()
    }));
    let mailbox = Arc::new(FakeMailbox::default());
    mailbox.push(
        "alice",
        "team-a",
        r#"{"type":"plan_approval_response","request_id":"plan_approval-alice-team-a-deadbeef","approved":true,"permission_mode":"accept_edits"}"#,
        "team-lead",
    );

    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    )
    .with_mailbox(mailbox.clone(), "alice".into(), "team-a".into(), true);

    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    let texts = history_texts(&h);
    // Approval reminder first, then Plan reminder (still in Plan mode).
    assert!(texts[0].contains("## Plan Approved"));
    // PermissionMode serializes as camelCase (canonical wire format).
    assert!(texts[0].contains("acceptEdits"));

    // app_state cleared.
    let guard = app_state.read().await;
    assert!(!guard.awaiting_plan_approval);
    assert!(guard.awaiting_plan_approval_request_id.is_none());
    // last_permission_mode tracks leader's override for next turn.
    assert_eq!(
        guard.last_permission_mode,
        Some(PermissionMode::AcceptEdits)
    );

    // Message was marked read.
    let marked = mailbox.marked.lock().unwrap();
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].0, "alice");
}

#[tokio::test]
async fn teammate_polls_approval_and_injects_rejected_reminder_with_feedback() {
    let app_state = Arc::new(RwLock::new(ToolAppState {
        awaiting_plan_approval_request_id: Some("plan_approval-alice-team-a-cafebabe".into()),
        awaiting_plan_approval: true,
        ..Default::default()
    }));
    let mailbox = Arc::new(FakeMailbox::default());
    mailbox.push(
        "alice",
        "team-a",
        r#"{"type":"plan_approval_response","request_id":"plan_approval-alice-team-a-cafebabe","approved":false,"feedback":"refine the security section"}"#,
        "team-lead",
    );

    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    )
    .with_mailbox(mailbox, "alice".into(), "team-a".into(), true);

    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    let texts = history_texts(&h);
    assert!(texts[0].contains("## Plan Rejected"));
    assert!(texts[0].contains("refine the security section"));
    // No mode override on rejection — last_permission_mode tracks the
    // *current* mode (Plan) via reconcile_mode_transition, not a new
    // target from the response. Approval would set it to the response's
    // `permission_mode` field (which rejection lacks).
    let guard = app_state.read().await;
    assert_ne!(
        guard.last_permission_mode,
        Some(PermissionMode::AcceptEdits)
    );
    assert_ne!(
        guard.last_permission_mode,
        Some(PermissionMode::BypassPermissions)
    );
}

#[tokio::test]
async fn teammate_ignores_unrelated_response_ids() {
    // Another teammate's response must not be consumed here.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        awaiting_plan_approval_request_id: Some("plan_approval-alice-team-a-mine".into()),
        awaiting_plan_approval: true,
        ..Default::default()
    }));
    let mailbox = Arc::new(FakeMailbox::default());
    mailbox.push(
        "alice",
        "team-a",
        r#"{"type":"plan_approval_response","request_id":"plan_approval-bob-team-a-theirs","approved":true}"#,
        "team-lead",
    );

    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    )
    .with_mailbox(mailbox, "alice".into(), "team-a".into(), true);

    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    // Plan reminder fires (still in Plan + still awaiting), but no
    // approval-consumed reminder.
    assert!(
        !texts.iter().any(|t| t.contains("## Plan Approved"))
            && !texts.iter().any(|t| t.contains("## Plan Rejected")),
        "unrelated request_id must not trigger approval consumption"
    );

    // app_state still awaiting.
    let guard = app_state.read().await;
    assert!(guard.awaiting_plan_approval);
}

#[tokio::test]
async fn team_lead_sees_pending_approvals_attachment() {
    let mailbox = Arc::new(FakeMailbox::default());
    mailbox.push(
        "team-lead",
        "team-a",
        r##"{"type":"plan_approval_request","from":"alice","planFilePath":"/tmp/alice.md","planContent":"# Alice's plan","requestId":"req-1"}"##,
        "alice",
    );
    mailbox.push(
        "team-lead",
        "team-a",
        r##"{"type":"plan_approval_request","from":"bob","planFilePath":"/tmp/bob.md","planContent":"# Bob's plan","requestId":"req-2"}"##,
        "bob",
    );

    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("leader-session".into()),
        None,
        Some(tmp.path().to_path_buf()),
        None,
    )
    .with_mailbox(mailbox.clone(), "team-lead".into(), "team-a".into(), false);

    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    let t = &texts[0];
    assert!(t.contains("## Pending Plan Approvals"));
    assert!(t.contains("alice"));
    assert!(t.contains("bob"));
    assert!(t.contains("req-1"));
    assert!(t.contains("req-2"));
    assert!(t.contains("Alice's plan"));
    assert!(t.contains("Bob's plan"));
    // Both requests marked read so next turn doesn't re-inject.
    let marked = mailbox.marked.lock().unwrap();
    assert_eq!(marked.len(), 2);
}

#[tokio::test]
async fn non_leader_agent_does_not_inject_pending_attachment() {
    let mailbox = Arc::new(FakeMailbox::default());
    mailbox.push(
        "alice",
        "team-a",
        r##"{"type":"plan_approval_request","from":"bob","planFilePath":"/tmp/bob.md","planContent":"# plan","requestId":"req-1"}"##,
        "bob",
    );
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        None,
    )
    .with_mailbox(mailbox, "alice".into(), "team-a".into(), false);

    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert!(
        !texts.iter().any(|t| t.contains("Pending Plan Approvals")),
        "only the team-lead identity scans pending approvals"
    );
}

#[tokio::test]
async fn plan_reminder_reports_missing_plan_file() {
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("no-plan-yet".into()),
        None,
        Some(tmp.path().to_path_buf()),
        None,
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert!(texts[0].contains("No plan file exists yet"));
}

// ── Reentry variant (TS: plan_mode_reentry) ──

#[tokio::test]
async fn reentry_variant_fires_when_session_previously_exited() {
    // has_exited_plan_mode=true is sticky from a prior Exit. On the NEXT
    // Plan-mode turn TS (`attachments.ts:1213-1239`) emits BOTH the Reentry
    // banner AND the normal Full/Sparse reminder.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        has_exited_plan_mode: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    let session_id = "reentry-s1";
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some(session_id.into()),
        None,
        Some(plans_dir),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 2, "Reentry banner + Full reminder on re-entry");
    assert!(texts[0].contains("## Re-entering Plan Mode"));
    // Second reminder is the normal cadence — Full on attachment #1.
    assert!(texts[1].contains("## Plan Workflow"));
}

#[tokio::test]
async fn reentry_suppressed_when_plan_file_missing() {
    // TS gates Reentry on `existingPlan !== null` (attachments.ts:1216).
    // Without an on-disk plan, only the normal Full reminder fires.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        has_exited_plan_mode: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("no-plan-on-disk".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(!texts[0].contains("## Re-entering Plan Mode"));
    assert!(texts[0].contains("## Plan Workflow"));
}

#[tokio::test]
async fn reentry_followed_by_throttled_sparse_on_subsequent_turns() {
    // Human turn 1: Reentry banner + Full (2 messages). Human turns 2-5
    // throttled. Human turn 6: Sparse (1 message). Total: 3 messages.
    // Tests the human-turn semantics — each new turn pushes a fresh
    // user message so `turn_start` bumps the counter.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        has_exited_plan_mode: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    let session_id = "reentry-throttle";
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some(session_id.into()),
        None,
        Some(plans_dir),
        Some(app_state),
    );
    let mut h = MessageHistory::new();

    h.messages
        .push(coco_messages::create_user_message("turn 1"));
    r.turn_start(&mut h).await; // Reentry + Full
    for i in 2..=5 {
        h.messages
            .push(coco_messages::create_user_message(&format!("turn {i}")));
        r.turn_start(&mut h).await; // throttled
    }
    h.messages
        .push(coco_messages::create_user_message("turn 6"));
    r.turn_start(&mut h).await; // Sparse

    let texts = history_texts(&h);
    assert_eq!(texts.len(), 3);
    assert!(texts[0].contains("## Re-entering Plan Mode"));
    assert!(texts[1].contains("## Plan Workflow"));
    assert!(texts[2].contains("Plan mode still active"));
}

#[tokio::test]
async fn reentry_not_used_when_never_exited() {
    // First plan-mode session in this run: should use Full, not Reentry.
    // Default struct already has `has_exited_plan_mode: false`.
    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("## Plan Workflow"));
    assert!(!texts[0].contains("## Re-entering"));
}

#[tokio::test]
async fn reentry_clears_has_exited_flag_after_firing() {
    // TS parity (`attachments.ts:1218`): emit Reentry → clear the flag.
    // Next time the user re-enters Plan, we should use Full, not Reentry.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        has_exited_plan_mode: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    let session_id = "reentry-clear-flag";
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some(session_id.into()),
        None,
        Some(plans_dir),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert!(texts[0].contains("## Re-entering Plan Mode"));

    // The flag is now cleared.
    let guard = app_state.read().await;
    assert!(
        !guard.has_exited_plan_mode,
        "has_exited_plan_mode must clear after emitting Reentry"
    );
}

#[tokio::test]
async fn unannounced_plan_exit_via_shift_tab_sets_flags() {
    // Simulate: last turn was Plan, this turn is Default (user toggled
    // Shift+Tab). Engine should behave as if ExitPlanModeTool ran —
    // set both exit-reminder flags. TS parity:
    // `transitionPermissionMode` fires `setHasExitedPlanMode(true)`.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        last_permission_mode: Some(PermissionMode::Plan),
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    // Exit reminder should have fired (one-shot, flag consumed).
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("## Exited Plan Mode"));

    // has_exited_plan_mode should now be true so a future re-entry
    // triggers the Reentry variant.
    let guard = app_state.read().await;
    assert!(guard.has_exited_plan_mode);
}

#[tokio::test]
async fn unannounced_plan_entry_via_shift_tab_clears_stale_exit_flag() {
    // Edge case: user rapidly Plan → Default → Plan via Shift+Tab.
    // The intermediate Default leg would have set the exit-attachment
    // flag. When we land back in Plan, we must clear it so we don't
    // spuriously emit an "exited plan mode" banner on this turn.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        last_permission_mode: Some(PermissionMode::Default),
        needs_plan_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;

    // Should be a Plan reminder, NOT an exit banner.
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1);
    assert!(
        !texts[0].contains("## Exited Plan Mode"),
        "Plan re-entry must clear the stale exit-attachment flag"
    );
    assert!(texts[0].contains("Plan mode"));
}

#[tokio::test]
async fn auto_mode_exit_banner_fires_once_when_flag_set() {
    // TS parity: when `needs_auto_mode_exit_attachment` is set (e.g. via
    // ExitPlanMode from a plan entered via Auto, or via a Shift+Tab
    // cycle reconciled on the next turn), the reminder emits exactly
    // one `## Exited Auto Mode` banner and clears the flag.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        needs_auto_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert_eq!(texts.len(), 1, "one banner fired");
    assert!(texts[0].contains("## Exited Auto Mode"));
    assert!(!app_state.read().await.needs_auto_mode_exit_attachment);

    // Second turn_start: no repeat.
    r.turn_start(&mut h).await;
    assert_eq!(
        history_texts(&h).len(),
        1,
        "one-shot — banner must not repeat",
    );
}

#[tokio::test]
async fn auto_mode_exit_banner_suppressed_while_still_in_auto() {
    // If the engine is currently in Auto, the exit banner would be a
    // lie — suppress it and clear the flag. TS parity:
    // `getAutoModeExitAttachment` early-exits when
    // `toolPermissionContext.mode === 'auto'`.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        needs_auto_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Auto,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert!(
        !texts.iter().any(|t| t.contains("## Exited Auto Mode")),
        "auto-mode-exit banner must be suppressed while still in Auto",
    );
    assert!(
        !app_state.read().await.needs_auto_mode_exit_attachment,
        "flag cleared even when suppressed",
    );
}

#[tokio::test]
async fn reconcile_auto_to_default_sets_auto_mode_exit_flag() {
    // Shift+Tab Auto → Default: reconcile_mode_transition on the next
    // `turn_start` observes the change via `last_permission_mode`.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        last_permission_mode: Some(PermissionMode::Auto),
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Default,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let texts = history_texts(&h);
    assert!(
        texts.iter().any(|t| t.contains("## Exited Auto Mode")),
        "Auto→Default cycle observed at turn start must emit the banner",
    );
}

#[tokio::test]
async fn reentering_auto_before_banner_clears_stale_flag() {
    // If the user toggled Auto → Default → Auto quickly, the exit
    // banner is stale — we're back in Auto. Reconcile must clear
    // the pending flag. TS parity: `permissionSetup.ts:1526` clears
    // the flag when re-activating Auto.
    let app_state = Arc::new(RwLock::new(ToolAppState {
        last_permission_mode: Some(PermissionMode::Default),
        needs_auto_mode_exit_attachment: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let mut r = PlanModeReminder::new(
        PermissionMode::Auto,
        Some("s1".into()),
        None,
        Some(tmp.path().to_path_buf()),
        Some(app_state.clone()),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    assert!(
        !app_state.read().await.needs_auto_mode_exit_attachment,
        "re-entering Auto must clear the stale exit flag"
    );
}

#[tokio::test]
async fn reentry_reports_existing_plan_when_file_exists() {
    let app_state = Arc::new(RwLock::new(ToolAppState {
        has_exited_plan_mode: true,
        ..Default::default()
    }));
    let tmp = tempdir().unwrap();
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    let session_id = "reentry-with-plan";
    coco_context::write_plan(session_id, &plans_dir, "# old plan", None).unwrap();

    let mut r = PlanModeReminder::new(
        PermissionMode::Plan,
        Some(session_id.into()),
        None,
        Some(plans_dir),
        Some(app_state),
    );
    let mut h = MessageHistory::new();
    r.turn_start(&mut h).await;
    let t = &history_texts(&h)[0];
    assert!(t.contains("A plan file exists at"));
    assert!(!t.contains("No plan file exists yet"));
}
