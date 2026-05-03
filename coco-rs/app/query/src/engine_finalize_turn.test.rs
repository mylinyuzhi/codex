use coco_subagent::{TaskNotification, TaskNotificationStatus, render_task_notification};
use pretty_assertions::assert_eq;

use super::render_teammate_message_wrapper;

#[test]
fn test_plain_message_uses_minimal_wrapper() {
    let out = render_teammate_message_wrapper("alice", "hello there");
    assert_eq!(
        out,
        "<teammate-message from=\"alice\">hello there</teammate-message>"
    );
}

#[test]
fn test_task_notification_surfaces_structured_attrs() {
    // Receive-side parser should extract task-id / status / summary
    // from a structured `<task-notification>` and lift them onto the
    // wrapper so the leader model can reason without re-parsing.
    let xml = render_task_notification(&TaskNotification {
        task_id: "agent-7af2",
        status: TaskNotificationStatus::Completed,
        summary: "Refactor done",
        result: Some("All 12 files updated"),
        usage: None,
    });

    let out = render_teammate_message_wrapper("worker-1", &xml);
    assert!(
        out.contains("task-id=\"agent-7af2\""),
        "task-id must be lifted: {out}"
    );
    assert!(
        out.contains("status=\"completed\""),
        "status must be lifted: {out}"
    );
    assert!(
        out.contains("summary=\"Refactor done\""),
        "summary must be lifted: {out}"
    );
    // Inner XML is preserved verbatim inside the wrapper for the
    // model that still wants to read result / usage detail.
    assert!(
        out.contains("<task-notification>"),
        "inner xml preserved: {out}"
    );
}

#[test]
fn test_failed_parse_falls_back_to_plain_wrapper() {
    // Looks like a notification (opens with `<task-notification>`)
    // but malformed inside — must not drop the message; fall back
    // to the plain wrapper so the leader still sees the content.
    let malformed = "<task-notification>something missing closing tags";
    let out = render_teammate_message_wrapper("worker-1", malformed);
    assert!(
        out.contains("<teammate-message from=\"worker-1\">"),
        "must use plain wrapper: {out}"
    );
    assert!(
        !out.contains("task-id="),
        "must not include structured attrs: {out}"
    );
    assert!(
        out.contains("something missing"),
        "content preserved: {out}"
    );
}

#[test]
fn test_killed_status_renders_as_killed() {
    let xml = render_task_notification(&TaskNotification {
        task_id: "agent-9",
        status: TaskNotificationStatus::Killed,
        summary: "Aborted by parent",
        result: None,
        usage: None,
    });
    let out = render_teammate_message_wrapper("w", &xml);
    assert!(
        out.contains("status=\"killed\""),
        "Killed → killed status attribute: {out}"
    );
}
