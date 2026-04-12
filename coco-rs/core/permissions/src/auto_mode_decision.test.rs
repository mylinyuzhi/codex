use coco_types::PermissionDecision;
use serde_json::json;

use super::*;
use crate::auto_mode_state::AutoModeState;
use crate::classifier::AutoModeRules;
use crate::denial_tracking::DenialTracker;

fn empty_rules() -> AutoModeRules {
    AutoModeRules {
        allow: vec![],
        soft_deny: vec![],
        environment: vec![],
    }
}

/// Mock classifier that always allows.
async fn mock_allow(_req: ClassifyRequest) -> Result<String, String> {
    Ok("<answer>allow</answer><reason>test allow</reason>".into())
}

/// Mock classifier that always blocks.
async fn mock_block(_req: ClassifyRequest) -> Result<String, String> {
    Ok("<answer>block</answer><reason>test block</reason>".into())
}

/// Mock classifier that errors.
async fn mock_error(_req: ClassifyRequest) -> Result<String, String> {
    Err("test error".into())
}

#[tokio::test]
async fn test_inactive_returns_none() {
    let state = AutoModeState::new();
    let mut tracker = DenialTracker::new();
    let result = can_use_tool_in_auto_mode(
        "Bash",
        &json!({"command": "ls"}),
        /*is_read_only*/ false,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_allow,
    )
    .await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_safe_tool_allows_without_classifier() {
    let state = AutoModeState::new();
    state.set_active(true);
    let mut tracker = DenialTracker::new();
    let result = can_use_tool_in_auto_mode(
        "Read",
        &json!({"file_path": "/tmp/test"}),
        /*is_read_only*/ true,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_error, // should never be called
    )
    .await;
    assert!(matches!(result, Some(PermissionDecision::Allow { .. })));
}

#[tokio::test]
async fn test_classifier_allow() {
    let state = AutoModeState::new();
    state.set_active(true);
    let mut tracker = DenialTracker::new();
    let result = can_use_tool_in_auto_mode(
        "WebFetch",
        &json!({"url": "https://example.com"}),
        /*is_read_only*/ true,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_allow,
    )
    .await;
    assert!(matches!(result, Some(PermissionDecision::Allow { .. })));
}

#[tokio::test]
async fn test_classifier_block() {
    let state = AutoModeState::new();
    state.set_active(true);
    let mut tracker = DenialTracker::new();
    let result = can_use_tool_in_auto_mode(
        "Bash",
        &json!({"command": "rm -rf /"}),
        /*is_read_only*/ false,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_block,
    )
    .await;
    assert!(matches!(result, Some(PermissionDecision::Deny { .. })));
    assert_eq!(tracker.consecutive_denials, 1);
}

#[tokio::test]
async fn test_circuit_breaker_fallthrough() {
    let state = AutoModeState::new();
    state.set_active(true);
    let mut tracker = DenialTracker::new();
    // Trip circuit breaker
    for _ in 0..3 {
        tracker.record_denial("Bash");
    }
    assert!(tracker.is_circuit_breaker_tripped());

    let result = can_use_tool_in_auto_mode(
        "Bash",
        &json!({"command": "ls"}),
        /*is_read_only*/ false,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_allow,
    )
    .await;
    assert!(result.is_none()); // Falls through to interactive
}

#[tokio::test]
async fn test_classifier_error_blocks() {
    let state = AutoModeState::new();
    state.set_active(true);
    let mut tracker = DenialTracker::new();
    let result = can_use_tool_in_auto_mode(
        "Bash",
        &json!({"command": "curl example.com"}),
        /*is_read_only*/ false,
        &state,
        &mut tracker,
        &[],
        &empty_rules(),
        mock_error,
    )
    .await;
    // Classifier error → blocks (safe default)
    assert!(matches!(result, Some(PermissionDecision::Deny { .. })));
}
