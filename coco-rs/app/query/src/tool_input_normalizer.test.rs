use serde_json::json;
use tempfile::tempdir;

use super::*;

#[test]
fn test_normalize_observable_tool_input_non_exit_tool_unchanged() {
    let input = json!({"file_path": "/tmp/a"});
    let normalized = normalize_observable_tool_input(
        "Read",
        input.clone(),
        ToolInputNormalizationContext::default(),
    );

    assert_eq!(normalized, input);
}

#[test]
fn test_normalize_observable_tool_input_exit_injects_plan_and_path() {
    let tmp = tempdir().unwrap();
    let plans_dir = tmp.path().join("plans");
    let session_id = "session-with-plan";
    coco_context::write_plan(session_id, &plans_dir, "## Plan\n- ship it", None).unwrap();

    let normalized = normalize_observable_tool_input(
        coco_types::ToolName::ExitPlanMode.as_str(),
        json!({"allowedPrompts": []}),
        ToolInputNormalizationContext {
            session_id: Some(session_id),
            plans_dir: Some(&plans_dir),
            agent_id: None,
        },
    );

    assert_eq!(normalized.get("plan"), Some(&json!("## Plan\n- ship it")));
    let path = normalized
        .get("planFilePath")
        .and_then(serde_json::Value::as_str)
        .expect("planFilePath injected");
    assert!(path.ends_with(".md"), "path: {path}");
    assert_eq!(normalized.get("allowedPrompts"), Some(&json!([])));
}

#[test]
fn test_normalize_observable_tool_input_exit_overrides_stale_plan() {
    let tmp = tempdir().unwrap();
    let plans_dir = tmp.path().join("plans");
    let session_id = "session-stale-plan";
    coco_context::write_plan(session_id, &plans_dir, "fresh plan", Some("agent-1")).unwrap();

    let normalized = normalize_observable_tool_input(
        coco_types::ToolName::ExitPlanMode.as_str(),
        json!({"plan": "stale", "planFilePath": "/tmp/stale.md"}),
        ToolInputNormalizationContext {
            session_id: Some(session_id),
            plans_dir: Some(&plans_dir),
            agent_id: Some("agent-1"),
        },
    );

    assert_eq!(normalized.get("plan"), Some(&json!("fresh plan")));
    let path = normalized
        .get("planFilePath")
        .and_then(serde_json::Value::as_str)
        .expect("planFilePath injected");
    assert!(path.contains("agent-agent-1"), "path: {path}");
}

#[test]
fn test_normalize_observable_tool_input_exit_without_plan_unchanged() {
    let tmp = tempdir().unwrap();
    let plans_dir = tmp.path().join("plans");
    let input = json!({"allowedPrompts": []});

    let normalized = normalize_observable_tool_input(
        coco_types::ToolName::ExitPlanMode.as_str(),
        input.clone(),
        ToolInputNormalizationContext {
            session_id: Some("missing-plan"),
            plans_dir: Some(&plans_dir),
            agent_id: None,
        },
    );

    assert_eq!(normalized, input);
}
