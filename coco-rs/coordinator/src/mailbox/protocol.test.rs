//! JSON round-trip tests for every `ProtocolMessage` variant + the
//! `is_structured_protocol_message` / `parse_protocol_message`
//! detection helpers.
//!
//! The protocol is wire-stable across teammate sessions (different
//! coco binaries can be reading and writing each other's mailbox
//! files), so the variant tags MUST stay byte-identical to TS. Each
//! test pins the exact `type` literal SDK clients see.

use super::*;
use pretty_assertions::assert_eq;

fn assert_round_trips(json: &str, expected_type: &str) {
    assert!(
        is_structured_protocol_message(json),
        "is_structured_protocol_message must accept: {json}"
    );
    let parsed = parse_protocol_message(json).expect("must parse");
    let serialized = serde_json::to_string(&parsed).expect("must serialise");
    let v: serde_json::Value = serde_json::from_str(&serialized).expect("must round-trip");
    assert_eq!(v["type"].as_str().unwrap(), expected_type);
}

#[test]
fn test_idle_notification_round_trip() {
    let json = r#"{"type":"idle_notification","from":"worker","timestamp":"2026-05-01T00:00:00Z","summary":"done"}"#;
    assert_round_trips(json, "idle_notification");
    let m = parse_protocol_message(json).unwrap();
    let ProtocolMessage::IdleNotification { from, summary, .. } = m else {
        panic!("wrong variant");
    };
    assert_eq!(from, "worker");
    assert_eq!(summary.as_deref(), Some("done"));
}

#[test]
fn test_permission_request_round_trip() {
    let json = r#"{"type":"permission_request","request_id":"r1","agent_id":"w@t","tool_name":"Bash","tool_use_id":"tu","description":"rm -rf /","input":{"command":"rm -rf /"}}"#;
    assert_round_trips(json, "permission_request");
}

#[test]
fn test_permission_response_round_trip() {
    let json = r#"{"type":"permission_response","request_id":"r1","subtype":"success"}"#;
    assert_round_trips(json, "permission_response");
}

#[test]
fn test_sandbox_permission_request_round_trip() {
    let json = r#"{"type":"sandbox_permission_request","request_id":"sb1","worker_id":"w@t","worker_name":"worker","host_pattern":{"host":"api.example.com"},"created_at":1700000000}"#;
    assert_round_trips(json, "sandbox_permission_request");
}

#[test]
fn test_sandbox_permission_response_round_trip() {
    let json = r#"{"type":"sandbox_permission_response","request_id":"sb1","host":"api.example.com","allow":true,"timestamp":"2026-05-01T00:00:00Z"}"#;
    assert_round_trips(json, "sandbox_permission_response");
}

#[test]
fn test_plan_approval_request_round_trip() {
    let json = r#"{"type":"plan_approval_request","from":"worker","timestamp":"t","plan_file_path":"","plan_content":"plan","request_id":"p1"}"#;
    assert_round_trips(json, "plan_approval_request");
}

#[test]
fn test_plan_approval_response_round_trip_with_feedback() {
    let json = r#"{"type":"plan_approval_response","request_id":"p1","approved":false,"feedback":"missing tests","timestamp":"t"}"#;
    assert_round_trips(json, "plan_approval_response");
    let m = parse_protocol_message(json).unwrap();
    let ProtocolMessage::PlanApprovalResponse {
        approved, feedback, ..
    } = m
    else {
        panic!("wrong variant");
    };
    assert!(!approved);
    assert_eq!(feedback.as_deref(), Some("missing tests"));
}

#[test]
fn test_shutdown_request_round_trip() {
    let json = r#"{"type":"shutdown_request","request_id":"s1","from":"team-lead","reason":"team disbanded","timestamp":"t"}"#;
    assert_round_trips(json, "shutdown_request");
}

#[test]
fn test_shutdown_approved_round_trip() {
    let json = r#"{"type":"shutdown_approved","request_id":"s1","from":"worker","timestamp":"t"}"#;
    assert_round_trips(json, "shutdown_approved");
}

#[test]
fn test_shutdown_rejected_round_trip() {
    let json = r#"{"type":"shutdown_rejected","request_id":"s1","from":"worker","reason":"mid task","timestamp":"t"}"#;
    assert_round_trips(json, "shutdown_rejected");
}

#[test]
fn test_task_assignment_round_trip() {
    let json = r#"{"type":"task_assignment","task_id":"t1","subject":"refactor","description":"split agent_handle.rs","assigned_by":"team-lead","timestamp":"t"}"#;
    assert_round_trips(json, "task_assignment");
}

#[test]
fn test_team_permission_update_round_trip() {
    let json = r#"{"type":"team_permission_update","permission_update":{"behavior":"allow","tool":"WebFetch"},"directory_path":"/proj","tool_name":"WebFetch"}"#;
    assert_round_trips(json, "team_permission_update");
}

#[test]
fn test_mode_set_request_round_trip() {
    let json = r#"{"type":"mode_set_request","mode":"plan","from":"team-lead"}"#;
    assert_round_trips(json, "mode_set_request");
}

#[test]
fn test_is_structured_rejects_plain_text() {
    assert!(!is_structured_protocol_message("hello world"));
    assert!(!is_structured_protocol_message("{not json"));
}

#[test]
fn test_is_structured_rejects_unknown_type_tag() {
    let json = r#"{"type":"something_else","data":1}"#;
    assert!(
        !is_structured_protocol_message(json),
        "unknown `type` tag must not register as a protocol message"
    );
}

#[test]
fn test_parse_returns_none_for_non_protocol_text() {
    assert!(parse_protocol_message("plain message").is_none());
    assert!(parse_protocol_message("").is_none());
}

#[test]
fn test_check_message_type_filter() {
    let json =
        r#"{"type":"plan_approval_response","request_id":"p1","approved":true,"timestamp":"t"}"#;
    assert!(check_message_type(json, "plan_approval_response").is_some());
    assert!(
        check_message_type(json, "permission_response").is_none(),
        "type filter must reject mismatched variants"
    );
}

#[test]
fn test_optional_fields_omitted_on_serialize_when_none() {
    let m = ProtocolMessage::IdleNotification {
        from: "w".into(),
        timestamp: "t".into(),
        idle_reason: None,
        summary: None,
        completed_task_id: None,
        completed_status: None,
        failure_reason: None,
    };
    let s = serde_json::to_string(&m).unwrap();
    assert!(!s.contains("idle_reason"), "None fields must skip: {s}");
    assert!(!s.contains("summary"));
}
