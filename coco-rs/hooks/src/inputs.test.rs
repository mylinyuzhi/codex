//! Wire-format parity tests for hook input structs.
//!
//! These tests pin the JSON shape of every event-specific input
//! against TS `coreSchemas.ts` so a future refactor that drops or
//! renames a field surfaces the regression here, not in production.

use super::*;
use serde_json::Value;
use serde_json::json;

fn base() -> BaseHookInput {
    BaseHookInput {
        session_id: "sess".into(),
        cwd: "/cwd".into(),
        transcript_path: "/tmp/t.json".into(),
        permission_mode: Some("default".into()),
        agent_id: None,
        agent_type: None,
    }
}

/// `transcript_path` is required in TS — emit it even when empty.
#[test]
fn base_hook_input_emits_transcript_path_when_empty() {
    let mut b = base();
    b.transcript_path = String::new();
    let v = serde_json::to_value(&b).unwrap();
    assert!(
        v.get("transcript_path").is_some(),
        "transcript_path must appear on the wire (TS marks it required)"
    );
    assert_eq!(v["transcript_path"], "");
}

/// `transcript_path` defaults to "" when missing on deserialize so
/// older fixtures keep parsing.
#[test]
fn base_hook_input_defaults_transcript_path() {
    let v: BaseHookInput = serde_json::from_value(json!({"session_id": "s", "cwd": "/c"})).unwrap();
    assert_eq!(v.transcript_path, "");
}

#[test]
fn pre_tool_use_carries_tool_use_id() {
    let input = PreToolUseInput {
        base: base(),
        hook_event_name: HookEventType::PreToolUse,
        tool_name: "Bash".into(),
        tool_input: json!({"command": "ls"}),
        tool_use_id: "tu-1".into(),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["hook_event_name"], "PreToolUse");
    assert_eq!(v["tool_name"], "Bash");
    assert_eq!(v["tool_use_id"], "tu-1");
    assert_eq!(v["tool_input"], json!({"command": "ls"}));
}

#[test]
fn post_tool_use_failure_emits_tool_use_id_and_is_interrupt() {
    let input = PostToolUseFailureInput {
        base: base(),
        hook_event_name: HookEventType::PostToolUseFailure,
        tool_name: "Bash".into(),
        tool_input: json!({"command": "false"}),
        tool_use_id: "tu-9".into(),
        error: "exit 1".into(),
        is_interrupt: Some(true),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["tool_use_id"], "tu-9");
    assert_eq!(v["error"], "exit 1");
    assert_eq!(v["is_interrupt"], true);
    // Rust-only fields like the old `error_type` should NOT be on the wire.
    assert!(v.get("error_type").is_none());
}

#[test]
fn post_tool_use_failure_omits_is_interrupt_when_none() {
    let input = PostToolUseFailureInput {
        base: base(),
        hook_event_name: HookEventType::PostToolUseFailure,
        tool_name: "Bash".into(),
        tool_input: json!({}),
        tool_use_id: "tu-9".into(),
        error: "boom".into(),
        is_interrupt: None,
    };
    let v = serde_json::to_value(&input).unwrap();
    assert!(v.get("is_interrupt").is_none());
}

#[test]
fn permission_denied_emits_tool_input_and_tool_use_id() {
    let input = PermissionDeniedInput {
        base: base(),
        hook_event_name: HookEventType::PermissionDenied,
        tool_name: "Bash".into(),
        tool_input: json!({"command": "rm -rf /"}),
        tool_use_id: "tu-2".into(),
        reason: "auto-mode denied".into(),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["tool_input"], json!({"command": "rm -rf /"}));
    assert_eq!(v["tool_use_id"], "tu-2");
    assert_eq!(v["reason"], "auto-mode denied");
}

#[test]
fn permission_request_omits_tool_use_id_and_carries_suggestions() {
    let input = PermissionRequestInput {
        base: base(),
        hook_event_name: HookEventType::PermissionRequest,
        tool_name: "Bash".into(),
        tool_input: json!({"command": "ls"}),
        permission_suggestions: Some(json!([{"behavior": "allow"}])),
    };
    let v = serde_json::to_value(&input).unwrap();
    // TS schema does NOT include tool_use_id on this event.
    assert!(
        v.get("tool_use_id").is_none(),
        "PermissionRequest must not emit tool_use_id (TS schema omits it)"
    );
    assert_eq!(v["permission_suggestions"], json!([{"behavior": "allow"}]));
}

#[test]
fn notification_carries_optional_title() {
    let input = NotificationInput {
        base: base(),
        hook_event_name: HookEventType::Notification,
        message: "hi".into(),
        title: Some("Heads up".into()),
        notification_type: "permission_prompt".into(),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["title"], "Heads up");
    assert_eq!(v["message"], "hi");
    assert_eq!(v["notification_type"], "permission_prompt");
}

#[test]
fn stop_input_emits_stop_hook_active() {
    let input = StopInput {
        base: base(),
        hook_event_name: HookEventType::Stop,
        stop_hook_active: true,
        last_assistant_message: Some("done".into()),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["stop_hook_active"], true);
    assert_eq!(v["last_assistant_message"], "done");
    // The old Rust-only `reason` field is gone.
    assert!(v.get("reason").is_none());
}

#[test]
fn subagent_stop_emits_stop_hook_active_and_required_transcript() {
    let input = SubagentStopInput {
        base: base(),
        hook_event_name: HookEventType::SubagentStop,
        stop_hook_active: false,
        agent_type: "Explore".into(),
        agent_id: "a-1".into(),
        agent_transcript_path: String::new(),
        last_assistant_message: None,
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["stop_hook_active"], false);
    assert_eq!(v["agent_transcript_path"], "");
    assert!(v.get("last_assistant_message").is_none());
}

#[test]
fn pre_compact_emits_null_for_missing_custom_instructions() {
    // TS `PreCompactHookInputSchema.custom_instructions: z.string().nullable()`
    // — the field MUST appear on the wire, with `null` when absent. Hooks
    // checking `input.custom_instructions === null` rely on this shape.
    let input = PreCompactInput {
        base: base(),
        hook_event_name: HookEventType::PreCompact,
        trigger: CompactTrigger::Auto,
        custom_instructions: None,
    };
    let v = serde_json::to_value(&input).unwrap();
    assert!(v.get("custom_instructions").is_some(), "must emit field");
    assert_eq!(v["custom_instructions"], Value::Null);
}

#[test]
fn pre_compact_emits_string_for_custom_instructions() {
    let input = PreCompactInput {
        base: base(),
        hook_event_name: HookEventType::PreCompact,
        trigger: CompactTrigger::Manual,
        custom_instructions: Some("focus on TODOs".into()),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["custom_instructions"], "focus on TODOs");
}

#[test]
fn post_compact_requires_compact_summary() {
    let input = PostCompactInput {
        base: base(),
        hook_event_name: HookEventType::PostCompact,
        trigger: CompactTrigger::Auto,
        compact_summary: "summary text".into(),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["compact_summary"], "summary text");
    // Reject the laxer combined struct: PostCompact must not have
    // a `custom_instructions` field on the wire.
    assert!(v.get("custom_instructions").is_none());
}

#[test]
fn elicitation_carries_optional_mode_url_id() {
    let input = ElicitationInput {
        base: base(),
        hook_event_name: HookEventType::Elicitation,
        mcp_server_name: "github".into(),
        message: "Authorize?".into(),
        mode: Some(ElicitationMode::Form),
        url: Some("https://example.com/auth".into()),
        elicitation_id: Some("e-1".into()),
        requested_schema: Some(json!({"type": "object"})),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["mode"], "form");
    assert_eq!(v["url"], "https://example.com/auth");
    assert_eq!(v["elicitation_id"], "e-1");
    assert_eq!(v["requested_schema"], json!({"type": "object"}));
}

#[test]
fn instructions_loaded_carries_required_memory_type_and_optional_metadata() {
    let input = InstructionsLoadedInput {
        base: base(),
        hook_event_name: HookEventType::InstructionsLoaded,
        file_path: "/p/CLAUDE.md".into(),
        memory_type: MemoryType::Project,
        load_reason: InstructionsLoadReason::SessionStart,
        globs: Some(vec!["**/*.rs".into()]),
        trigger_file_path: Some("/p/src/main.rs".into()),
        parent_file_path: None,
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["memory_type"], "Project");
    assert_eq!(v["load_reason"], "session_start");
    assert_eq!(v["globs"], json!(["**/*.rs"]));
    assert_eq!(v["trigger_file_path"], "/p/src/main.rs");
    assert!(v.get("parent_file_path").is_none());
}

#[test]
fn task_created_input_has_task_subject_not_task_type() {
    let input = TaskCreatedInput {
        base: base(),
        hook_event_name: HookEventType::TaskCreated,
        task_id: "t-1".into(),
        task_subject: "Refactor compaction".into(),
        task_description: Some("Split CompactHookInput".into()),
        teammate_name: None,
        team_name: None,
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["task_subject"], "Refactor compaction");
    assert_eq!(v["task_description"], "Split CompactHookInput");
    // The old shared TaskEventInput's `task_type` field is gone.
    assert!(v.get("task_type").is_none());
}

#[test]
fn teammate_idle_carries_teammate_and_team_names() {
    let input = TeammateIdleInput {
        base: base(),
        hook_event_name: HookEventType::TeammateIdle,
        teammate_name: "Alice".into(),
        team_name: "frontend".into(),
    };
    let v = serde_json::to_value(&input).unwrap();
    assert_eq!(v["teammate_name"], "Alice");
    assert_eq!(v["team_name"], "frontend");
    // Old shape's task_id leak guard.
    assert!(v.get("task_id").is_none());
}

#[test]
fn enum_typed_fields_serialize_to_ts_wire_strings() {
    // Spot-check every typed-enum field renders its TS literal value.
    assert_eq!(
        serde_json::to_value(SessionStartSource::Startup).unwrap(),
        "startup"
    );
    assert_eq!(
        serde_json::to_value(SessionStartSource::Compact).unwrap(),
        "compact"
    );
    assert_eq!(serde_json::to_value(SetupTrigger::Init).unwrap(), "init");
    assert_eq!(
        serde_json::to_value(SetupTrigger::Maintenance).unwrap(),
        "maintenance"
    );
    assert_eq!(
        serde_json::to_value(CompactTrigger::Manual).unwrap(),
        "manual"
    );
    assert_eq!(serde_json::to_value(CompactTrigger::Auto).unwrap(), "auto");
    assert_eq!(serde_json::to_value(ExitReason::Clear).unwrap(), "clear");
    assert_eq!(
        serde_json::to_value(ExitReason::PromptInputExit).unwrap(),
        "prompt_input_exit"
    );
    assert_eq!(
        serde_json::to_value(ExitReason::BypassPermissionsDisabled).unwrap(),
        "bypass_permissions_disabled"
    );
    assert_eq!(
        serde_json::to_value(FileChangeEvent::Change).unwrap(),
        "change"
    );
    assert_eq!(
        serde_json::to_value(FileChangeEvent::Unlink).unwrap(),
        "unlink"
    );
    assert_eq!(
        serde_json::to_value(ConfigChangeSource::PolicySettings).unwrap(),
        "policy_settings"
    );
    // PascalCase variant: TS uses 'User'/'Project'/'Local'/'Managed'
    // for memory_type — Rust default serde rename keeps PascalCase.
    assert_eq!(
        serde_json::to_value(MemoryType::Project).unwrap(),
        "Project"
    );
    assert_eq!(
        serde_json::to_value(MemoryType::Managed).unwrap(),
        "Managed"
    );
    assert_eq!(
        serde_json::to_value(InstructionsLoadReason::SessionStart).unwrap(),
        "session_start"
    );
    assert_eq!(
        serde_json::to_value(InstructionsLoadReason::PathGlobMatch).unwrap(),
        "path_glob_match"
    );
    assert_eq!(serde_json::to_value(ElicitationMode::Form).unwrap(), "form");
    assert_eq!(
        serde_json::to_value(ElicitationAction::Accept).unwrap(),
        "accept"
    );
}

#[test]
fn enum_wire_strs_match_serde_output() {
    // The manual `as_wire_str()` must produce the exact same string
    // that serde serializes — otherwise hook matchers (which key off
    // `as_wire_str`) would diverge from the JSON the hook reads.
    fn assert_match<T: serde::Serialize>(value: &T, wire: &str) {
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!(wire)
        );
    }
    assert_match(
        &SessionStartSource::Resume,
        SessionStartSource::Resume.as_wire_str(),
    );
    assert_match(
        &SetupTrigger::Maintenance,
        SetupTrigger::Maintenance.as_wire_str(),
    );
    assert_match(&CompactTrigger::Auto, CompactTrigger::Auto.as_wire_str());
    assert_match(&ExitReason::Other, ExitReason::Other.as_wire_str());
    assert_match(&FileChangeEvent::Add, FileChangeEvent::Add.as_wire_str());
    assert_match(
        &ConfigChangeSource::UserSettings,
        ConfigChangeSource::UserSettings.as_wire_str(),
    );
    assert_match(&MemoryType::Local, MemoryType::Local.as_wire_str());
    assert_match(
        &InstructionsLoadReason::Include,
        InstructionsLoadReason::Include.as_wire_str(),
    );
    assert_match(&ElicitationMode::Url, ElicitationMode::Url.as_wire_str());
    assert_match(
        &ElicitationAction::Decline,
        ElicitationAction::Decline.as_wire_str(),
    );
}
