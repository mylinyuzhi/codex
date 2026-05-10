use std::sync::Arc;

use coco_inference::ToolCallPart;
use coco_tool_runtime::CanUseToolCallContext;
use coco_tool_runtime::CanUseToolDecision;
use coco_tool_runtime::CanUseToolHandle;
use coco_tool_runtime::DecisionReason;
use coco_tool_runtime::ToolUseContext;
use coco_types::PermissionBehavior;
use coco_types::PermissionDecision;
use coco_types::PermissionDecisionReason;
use serde_json::json;

use super::*;

#[derive(Debug)]
struct AlwaysDenyHandle;

#[async_trait::async_trait]
impl CanUseToolHandle for AlwaysDenyHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &CanUseToolCallContext,
    ) -> CanUseToolDecision {
        CanUseToolDecision::Deny {
            message: "fork blocked tool".into(),
            decision_reason: DecisionReason::Other {
                reason: "fork_policy".into(),
            },
        }
    }
}

#[derive(Debug)]
struct AlwaysAllowRewriteHandle;

#[async_trait::async_trait]
impl CanUseToolHandle for AlwaysAllowRewriteHandle {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &CanUseToolCallContext,
    ) -> CanUseToolDecision {
        CanUseToolDecision::Allow {
            updated_input: Some(json!({"file_path": "/overlay/foo.txt"})),
            decision_reason: DecisionReason::Other {
                reason: "rewrite".into(),
            },
        }
    }
}

fn tool_call() -> ToolCallPart {
    ToolCallPart {
        tool_call_id: "call-1".into(),
        tool_name: "Read".into(),
        input: json!({"file_path": "/main/foo.txt"}),
        provider_executed: None,
        provider_metadata: None,
    }
}

#[tokio::test]
async fn test_can_use_tool_deny_becomes_permission_deny_in_preparer() {
    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(Arc::new(AlwaysDenyHandle));

    let resolution = resolve_can_use_tool_decision(
        &tool_call(),
        &json!({"file_path": "/main/foo.txt"}),
        &ctx,
        None,
    )
    .await
    .expect("canUseTool decision should run");

    match resolution {
        CanUseToolResolution::Decision(PermissionDecision::Deny { message, reason }) => {
            assert_eq!(message, "fork blocked tool");
            match reason {
                PermissionDecisionReason::AsyncAgent { reason } => {
                    assert_eq!(reason, "fork_policy");
                }
                other => panic!("expected AsyncAgent reason, got {other:?}"),
            }
        }
        CanUseToolResolution::Decision(other) => {
            panic!("expected Deny decision, got {other:?}");
        }
        CanUseToolResolution::Ask => panic!("expected concrete Deny decision"),
    }
}

#[tokio::test]
async fn test_can_use_tool_allow_rewrite_becomes_permission_allow() {
    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(Arc::new(AlwaysAllowRewriteHandle));

    let resolution = resolve_can_use_tool_decision(
        &tool_call(),
        &json!({"file_path": "/main/foo.txt"}),
        &ctx,
        None,
    )
    .await
    .expect("canUseTool decision should run");

    match resolution {
        CanUseToolResolution::Decision(PermissionDecision::Allow {
            updated_input,
            feedback,
        }) => {
            assert_eq!(
                updated_input,
                Some(json!({"file_path": "/overlay/foo.txt"}))
            );
            assert_eq!(feedback.as_deref(), Some("rewrite"));
        }
        CanUseToolResolution::Decision(other) => {
            panic!("expected Allow decision, got {other:?}");
        }
        CanUseToolResolution::Ask => panic!("expected concrete Allow decision"),
    }
}

#[tokio::test]
async fn test_hook_allow_bypasses_can_use_tool_unless_required() {
    let mut ctx = ToolUseContext::test_default();
    ctx.can_use_tool = Some(Arc::new(AlwaysDenyHandle));

    let skipped = resolve_can_use_tool_decision(
        &tool_call(),
        &json!({"file_path": "/main/foo.txt"}),
        &ctx,
        Some(PermissionBehavior::Allow),
    )
    .await;
    assert!(
        skipped.is_none(),
        "normal hook allow should preserve existing auto-approve semantics"
    );

    ctx.require_can_use_tool = true;
    let enforced = resolve_can_use_tool_decision(
        &tool_call(),
        &json!({"file_path": "/main/foo.txt"}),
        &ctx,
        Some(PermissionBehavior::Allow),
    )
    .await;
    assert!(
        matches!(
            enforced,
            Some(CanUseToolResolution::Decision(
                PermissionDecision::Deny { .. }
            ))
        ),
        "require_can_use_tool must force the fork policy to run"
    );
}
